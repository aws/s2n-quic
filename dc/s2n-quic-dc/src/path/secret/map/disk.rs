// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module implements on-disk persistence for the path secret map.
//!
//! Only part of the information is persisted (today, entry socket addresses and credential IDs).

use std::{
    fmt,
    fs::File,
    io::{self, BufWriter, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    path::{Path, PathBuf},
    sync::{Mutex, Weak},
    time::{Duration, SystemTime},
};

use crate::{
    credentials,
    path::secret::map::{cleaner::CLEANER_CYCLE, Entry, Epoch},
};

const HEADER: &str = "s2n-quic-dc path secret map";

/// The version identifier written immediately after the [`HEADER`].
const VERSION: &[u8] = b"v1";

/// Maximum size of a persisted file we are willing to read into memory.
const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Stop adding new entries to a serialized file once it grows past this size. Kept a margin below
/// [`MAX_FILE_SIZE`] so a file we write is always comfortably within the size we're willing to read
/// back, even if the last entry pushes us slightly over the threshold.
const MAX_SERIALIZED_SIZE: u64 = MAX_FILE_SIZE - 1024 * 1024;

/// A writer that tracks how many bytes have been written through it.
///
/// We use this during serialization to stop adding entries once the file reaches its size cap,
/// without having to predict each entry's encoded length by hand.
struct CountingWriter<W> {
    inner: W,
    bytes_written: u64,
}

impl<W: Write> CountingWriter<W> {
    fn new(inner: W) -> Self {
        CountingWriter {
            inner,
            bytes_written: 0,
        }
    }

    /// The total number of bytes written through this writer so far.
    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn new_writer(tmp_path: &Path) -> io::Result<CountingWriter<BufWriter<File>>> {
    // We assume that we are the unique owner of the file path, i.e., there's no attempt to
    // synchronize with other writers attempting to write to it. We do take some effort to
    // avoid half-writing a file though.
    let output = std::fs::File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(tmp_path)?;

    let mut output = CountingWriter::new(std::io::BufWriter::new(output));

    output.write_all(HEADER.as_bytes())?;

    Ok(output)
}

/// Converts a wall-clock `duration` into a number of access-time epochs (which advance once per
/// [`CLEANER_CYCLE`]), rounding to the nearest epoch with a floor of one for any non-zero duration.
fn duration_to_epochs(duration: Duration) -> u64 {
    if duration.is_zero() {
        return 0;
    }
    let cycle = CLEANER_CYCLE.as_secs_f64();
    ((duration.as_secs_f64() / cycle).round() as u64).max(1)
}

/// Builds a [`Serializer`].
///
/// Constructed via [`Serializer::builder`].
#[derive(Clone, Debug)]
pub struct SerializerBuilder {
    path: PathBuf,
    period: Option<Duration>,
    max_idle_epochs: Option<u64>,
}

impl SerializerBuilder {
    /// Enables periodic serialization, roughly once per `period`.
    ///
    /// The cleaner drives serialization on a ~once-per-minute cadence, so `period` is rounded down
    /// to a whole number of minutes (with a floor of one minute) and jittered automatically.
    ///
    /// Regardless of this period, callers can use [`Map::serialize_to_disk`] to serialize now.
    pub fn with_period(mut self, period: Duration) -> Self {
        self.period = Some(period);
        self
    }

    /// Restricts serialization to entries accessed within the last `duration`.
    ///
    /// Note that the current implementation measures access times with roughly one minute
    /// granularity, so `duration` is rounded to the nearest whole number of epochs (at least one
    /// for any non-zero duration).
    pub fn with_max_idle(mut self, duration: Duration) -> Self {
        self.max_idle_epochs = Some(duration_to_epochs(duration));
        self
    }

    /// Finalizes the configuration into a [`Serializer`].
    pub fn build(self) -> io::Result<Serializer> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            if !parent.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "serializer destination directory {} does not exist",
                        parent.display()
                    ),
                ));
            }
        }

        Ok(Serializer {
            path: self.path,
            period: self.period,
            max_idle_epochs: self.max_idle_epochs,
            write_lock: Mutex::new(()),
        })
    }
}

/// Configuration for persisting the path secret map to disk.
pub struct Serializer {
    /// Where the serialized map is written.
    path: PathBuf,
    /// If set, the cleaner serializes the map roughly once per this duration (jittered).
    period: Option<Duration>,
    /// If set, entries idle for more than this many epochs are excluded from serialization.
    ///
    /// Access time is tracked at epoch granularity (epochs advance roughly once a minute), so this
    /// filters by recency of use. `None` writes every still-live entry regardless of access time.
    max_idle_epochs: Option<u64>,
    /// Serializes concurrent writes. The background cleaner and ad-hoc callers share the same
    /// destination (and `.tmp` staging file), so we hold this for the duration of a write to
    /// avoid two serializations clobbering each other's temporary file or racing the rename.
    write_lock: Mutex<()>,
}

impl fmt::Debug for Serializer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Serializer")
            .field("path", &self.path)
            .field("period", &self.period)
            .field("max_idle_epochs", &self.max_idle_epochs)
            .finish()
    }
}

impl Serializer {
    /// Begins configuring a serializer that writes to `path`.
    ///
    /// `path` should take the form `some_dir/file_name`: the parent directory (`some_dir`) must
    /// already exist and the final component (`file_name`) names the file that will be written.
    /// Note that a temporary file will be written at `some_dir/file_name.tmp` during each
    /// serialization and then renamed into the passed file path. Currently the periodic
    /// serialization will not attempt to enforce disk persistence (e.g. via fsync) or perform any
    /// error-checking (e.g. checksums) for consistency of the written data.
    ///
    /// A default builder will serialize all entries only on-demand (via
    /// [`Map::serialize_to_disk`]).
    pub fn builder(path: impl Into<PathBuf>) -> SerializerBuilder {
        SerializerBuilder {
            path: path.into(),
            period: None,
            max_idle_epochs: None,
        }
    }

    /// The path the serialized map is written to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The configured periodic serialization interval, if any.
    pub fn period(&self) -> Option<Duration> {
        self.period
    }

    /// The lowest access epoch that will be serialized given `current_epoch`, or `None` if every
    /// entry should be written regardless of access time.
    fn min_epoch(&self, current_epoch: Epoch) -> Option<u64> {
        self.max_idle_epochs
            .map(|window| current_epoch.get().saturating_sub(window))
    }

    /// Writes the entries in `entries` to the configured path, filtering by recency of access
    /// relative to `current_epoch`.
    ///
    /// `entries` is iterated and any still-live entry passing the recency filter is written.
    /// Returns the number of entries written and the resulting file size.
    ///
    /// This is `pub(crate)` because it references the crate-internal [`Entry`] and [`Epoch`] types;
    /// callers outside the crate drive serialization through the path secret map builder instead.
    pub(crate) fn serialize(
        &self,
        entries: &[Weak<Entry>],
        current_epoch: Epoch,
    ) -> io::Result<SerializeStats> {
        self.serialize_with_max_size(entries, current_epoch, MAX_SERIALIZED_SIZE)
    }

    /// As [`Serializer::serialize`], but stops adding entries once the file grows past `max_size`.
    /// Exposed separately so tests can exercise the size cap without writing tens of megabytes.
    fn serialize_with_max_size(
        &self,
        entries: &[Weak<Entry>],
        current_epoch: Epoch,
        max_size: u64,
    ) -> io::Result<SerializeStats> {
        // Hold the write lock for the whole operation so a concurrent serialization (e.g. the
        // background cleaner racing an ad-hoc call) can't clobber our `.tmp` file or rename.
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());

        let min_epoch = self.min_epoch(current_epoch);

        // Write to a temporary file and rename it into place once complete, so a reader never
        // observes a half-written file.
        let tmp_path = self.path.with_extension("tmp");
        let mut output = new_writer(&tmp_path)?;

        // Write a version identifier
        output.write_all(VERSION)?;

        // Record when we started writing this file, as seconds since the Unix epoch. Times before
        // the epoch (or arithmetic errors) are clamped to 0.
        let started_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        output.write_all(&started_at.to_le_bytes())?;

        let mut written = 0;
        for entry in entries.iter() {
            // Stop adding new entries once the file has grown past the maximum serialized size.
            // Entries are tiny (tens of bytes) relative to the margin we keep below MAX_FILE_SIZE,
            // so checking after the fact rather than predicting each entry's size is fine.
            if output.bytes_written() > max_size {
                break;
            }

            let Some(entry) = entry.upgrade() else {
                continue;
            };

            // Skip entries idle for longer than the configured window.
            if min_epoch.is_some_and(|min| entry.accessed_at_epoch().get() < min) {
                continue;
            }

            written += 1;
            let peer = entry.peer();
            match peer {
                SocketAddr::V4(addr) => {
                    output.write_all(&[0])?;
                    output.write_all(&addr.ip().octets())?;
                    output.write_all(&addr.port().to_le_bytes())?;
                }
                SocketAddr::V6(addr) => {
                    let minimal = addr.flowinfo() == 0 && addr.scope_id() == 0;
                    if minimal {
                        output.write_all(&[1])?;
                    } else {
                        output.write_all(&[2])?;
                    }
                    output.write_all(&addr.ip().octets())?;
                    output.write_all(&addr.port().to_le_bytes())?;

                    if !minimal {
                        output.write_all(&addr.flowinfo().to_le_bytes())?;
                        output.write_all(&addr.scope_id().to_le_bytes())?;
                    }
                }
            }

            output.write_all(&entry.id()[..])?;
        }

        output.flush()?;

        let file_size = output.bytes_written();

        std::fs::rename(&tmp_path, &self.path)?;

        Ok(SerializeStats {
            entries: written,
            file_size,
        })
    }
}

/// Statistics about a completed serialization.
#[derive(Copy, Clone, Debug)]
pub(crate) struct SerializeStats {
    /// The number of entries written to the file.
    pub(crate) entries: usize,
    /// The total size of the written file, in bytes.
    pub(crate) file_size: u64,
}

/// A single entry read back from a persisted file.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
#[non_exhaustive]
pub struct DiskEntry {
    pub peer: SocketAddr,
    pub id: credentials::Id,
}

/// Reads the file at `path` fully into memory and returns an iterator yielding the entries it
/// contains.
///
/// The file is rejected (without being read) if it is larger than [`MAX_FILE_SIZE`]. The header,
/// version, and timestamp are validated up front; per-entry decoding happens lazily as the
/// returned iterator is advanced, so a truncated or corrupt entry surfaces as an `Err` item rather
/// than failing the whole call.
pub fn deserialize(path: &Path) -> io::Result<Entries> {
    let file = File::open(path)?;

    let len = file.metadata()?.len();
    if len > MAX_FILE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is {len} bytes, larger than the maximum of {MAX_FILE_SIZE} bytes"),
        ));
    }

    let mut bytes = Vec::with_capacity(len as usize);
    file.take(len).read_to_end(&mut bytes)?;

    let mut reader = Reader { bytes: &bytes };

    // Validate the header and version before handing the rest off to the iterator.
    if reader.take(HEADER.len())? != HEADER.as_bytes() {
        return Err(invalid_data("missing or invalid header"));
    }
    if reader.take(VERSION.len())? != VERSION {
        return Err(invalid_data("missing or unsupported version"));
    }

    let started_at = {
        let secs = u64::from_le_bytes(reader.take_array::<8>()?);
        // Guard against a corrupt timestamp: `SystemTime + Duration` panics on overflow, so reject
        // an out-of-range value as invalid data rather than aborting the whole read.
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(secs))
            .ok_or_else(|| invalid_data("started_at timestamp out of range"))?
    };

    // The entries begin wherever validation left the cursor.
    let pos = bytes.len() - reader.bytes.len();
    Ok(Entries {
        bytes,
        pos,
        started_at,
    })
}

/// Iterator over the entries in a persisted file.
///
/// Owns the bytes read from disk and decodes one [`DiskEntry`] per call to [`Iterator::next`].
pub struct Entries {
    bytes: Vec<u8>,
    pos: usize,
    /// The time at which the file started being written, as recorded in its header.
    pub started_at: SystemTime,
}

impl Iterator for Entries {
    type Item = io::Result<DiskEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.bytes.len() {
            return None;
        }

        let mut reader = Reader {
            bytes: &self.bytes[self.pos..],
        };
        let result = read_entry(&mut reader);
        // Advance past whatever was consumed. On error we jump to the end so iteration stops
        // rather than spinning on the same malformed bytes.
        self.pos = if result.is_ok() {
            self.bytes.len() - reader.bytes.len()
        } else {
            self.bytes.len()
        };
        Some(result)
    }
}

/// Decodes a single entry from `reader`.
fn read_entry(reader: &mut Reader) -> io::Result<DiskEntry> {
    let tag = reader.take(1)?[0];
    let peer = match tag {
        0 => {
            let ip = Ipv4Addr::from(reader.take_array::<4>()?);
            let port = u16::from_le_bytes(reader.take_array::<2>()?);
            SocketAddr::V4(SocketAddrV4::new(ip, port))
        }
        1 | 2 => {
            let ip = Ipv6Addr::from(reader.take_array::<16>()?);
            let port = u16::from_le_bytes(reader.take_array::<2>()?);
            let (flowinfo, scope_id) = if tag == 2 {
                (
                    u32::from_le_bytes(reader.take_array::<4>()?),
                    u32::from_le_bytes(reader.take_array::<4>()?),
                )
            } else {
                (0, 0)
            };
            SocketAddr::V6(SocketAddrV6::new(ip, port, flowinfo, scope_id))
        }
        other => return Err(invalid_data(format!("unknown peer tag {other}"))),
    };

    let id = credentials::Id::from(reader.take_array::<16>()?);

    Ok(DiskEntry { peer, id })
}

/// A cursor over an in-memory byte buffer.
///
/// `bytes` always holds the not-yet-consumed remainder; `take` peels prefixes off the front.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Returns the next `n` bytes and advances the cursor, or an error if fewer than `n` remain.
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        self.bytes
            .split_off(..n)
            .ok_or_else(|| invalid_data("unexpected end of file"))
    }

    /// Reads exactly `N` bytes and advances the cursor, returning them as a fixed-size array.
    #[expect(
        clippy::unwrap_in_result,
        reason = "take(N) always yields exactly N bytes"
    )]
    fn take_array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        Ok(self
            .take(N)?
            .try_into()
            .expect("take(N) always returns N bytes"))
    }
}

fn invalid_data(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

#[cfg(test)]
mod test;

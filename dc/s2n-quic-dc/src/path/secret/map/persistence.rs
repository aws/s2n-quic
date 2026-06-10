// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! On-disk persistence of `(credential_id, peer)` pairs and replay of
//! Unknown-Path-Secret control packets at server startup ("Server HI").
//!
//! See `knowledge/designs/server-hi-ups-redesign/` for the design rationale.

use crate::{
    credentials::Id,
    event::{self, EndpointPublisher as _, IntoEvent as _},
    packet::secret_control::unknown_path_secret::UnknownPathSecret,
    path::secret::stateless_reset,
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::inet::SocketAddress;
use std::{
    fs,
    io::{self, BufWriter, Write as _},
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

/// Per-process counter so multiple `Map`s in the same process get unique file names.
static MAP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Errors surfaced from `replay`. Capture-side IO failures are *not* surfaced —
/// they are observed via tracing + counters and never propagate, since failing
/// the cleaner cycle would degrade live traffic.
#[derive(Debug)]
pub enum ReplayError {
    Io(io::Error),
}

impl From<io::Error> for ReplayError {
    fn from(e: io::Error) -> Self {
        ReplayError::Io(e)
    }
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// Line-format helpers. Each persisted entry is one line:
///   `<credential_id_hex> <peer_addr>\n`
/// where credential_id_hex is exactly 32 lowercase hex chars (16 bytes) and
/// peer_addr is `SocketAddr::Display` (e.g. `10.0.0.1:5000` or `[::1]:5000`).
mod format {
    use super::*;

    pub fn write_line<W: io::Write>(w: &mut W, id: &Id, peer: &SocketAddr) -> io::Result<()> {
        for byte in id.iter() {
            write!(w, "{byte:02x}")?;
        }
        writeln!(w, " {peer}")
    }

    pub fn parse_line(s: &str) -> Option<(Id, SocketAddr)> {
        let s = s.trim_end_matches('\n').trim_end_matches('\r');
        let (hex, rest) = s.split_once(' ')?;
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let hi = hex.as_bytes()[i * 2];
            let lo = hex.as_bytes()[i * 2 + 1];
            *byte = (hex_nibble(hi)? << 4) | hex_nibble(lo)?;
        }
        let peer: SocketAddr = rest.parse().ok()?;
        Some((Id::from(bytes), peer))
    }

    fn hex_nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
}

/// Single-file snapshot writer. One per `PathSecretMap` instance.
///
/// File name: `peers-<pid>-<n>.txt` where `n` is a process-local counter so
/// multiple Maps in the same process don't collide.
///
/// The cleaner thread calls `write_snapshot` once per cleaner cycle with all
/// live entries; the writer does tmp-file + fsync + atomic rename.
pub struct Writer {
    file_path: PathBuf,
    tmp_path: PathBuf,
}

impl Writer {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        // Best-effort cleanup of orphan tmp files from a prior crash between
        // tmp-write and rename. Failure to read the dir is fatal (something is
        // really wrong with the path); failure to unlink an individual entry
        // is tolerated.
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "tmp") {
                let _ = fs::remove_file(&path);
            }
        }
        let pid = std::process::id();
        let n = MAP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let file_name = format!("peers-{pid}-{n}.txt");
        Ok(Self {
            file_path: dir.join(&file_name),
            tmp_path: dir.join(format!("{file_name}.tmp")),
        })
    }

    /// Write the full snapshot. Builds `tmp`, fsyncs, atomic-renames.
    /// Returns `Err` only if the OS fails the call; the caller (cleaner) logs
    /// at WARN and increments a counter rather than propagating.
    pub fn write_snapshot<I>(&self, entries: I) -> io::Result<()>
    where
        I: IntoIterator<Item = (Id, SocketAddr)>,
    {
        let f = fs::File::create(&self.tmp_path)?;
        let mut buf = BufWriter::new(&f);
        for (id, peer) in entries {
            format::write_line(&mut buf, &id, &peer)?;
        }
        buf.flush()?;
        f.sync_all()?;
        fs::rename(&self.tmp_path, &self.file_path)?;
        Ok(())
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}

/// Result of a replay invocation. Counters are exposed for the caller's
/// observability layer; the function is intentionally small.
#[derive(Debug, Default)]
pub struct ReplayCounts {
    pub sent: u64,
    pub send_errors: u64,
    pub timed_out: u64,
}

/// Walks `root_dir` for `peers-*.txt`, dedupes by credential_id (last-wins),
/// emits one Unknown-Path-Secret packet per entry on `socket` paced at
/// `rate_pps`. Stops at `timeout`. After successful walk, deletes consumed
/// files so the next incarnation starts fresh.
///
/// Note: `*.tmp` files are skipped (they're either orphan or in-flight).
pub fn replay<S>(
    root_dir: &Path,
    rate_pps: u32,
    timeout: Duration,
    signer: &stateless_reset::Signer,
    socket: &UdpSocket,
    subscriber: &S,
) -> Result<ReplayCounts, ReplayError>
where
    S: event::Subscriber,
{
    let mut counts = ReplayCounts::default();

    if !root_dir.exists() {
        return Ok(counts);
    }

    // Collect file paths first, then read each one. Last-wins dedup by id.
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(root_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with("peers-") && name.ends_with(".txt") {
            files.push(path);
        }
    }

    let mut entries: std::collections::HashMap<Id, SocketAddr> = std::collections::HashMap::new();
    for path in &files {
        let body = fs::read_to_string(path)?;
        for line in body.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some((id, peer)) = format::parse_line(line) {
                entries.insert(id, peer);
            }
        }
    }

    let deadline = Instant::now() + timeout;
    let sleep_per_packet = if rate_pps == 0 {
        Duration::ZERO
    } else {
        Duration::from_secs(1) / rate_pps
    };

    for (id, peer) in entries.into_iter() {
        if Instant::now() >= deadline {
            counts.timed_out += 1;
            continue;
        }
        let packet = UnknownPathSecret {
            wire_version: crate::packet::WireVersion::ZERO,
            credential_id: id,
            queue_id: None,
        };
        let stateless_reset_tag = signer.sign(&id);
        let mut buf = [0u8; UnknownPathSecret::MAX_PACKET_SIZE];
        let len = packet.encode(EncoderBuffer::new(&mut buf), &stateless_reset_tag);
        match socket.send_to(&buf[..len], peer) {
            Ok(_) => {
                counts.sent += 1;
                publish_sent(subscriber, &id, &peer);
            }
            Err(_) => {
                counts.send_errors += 1;
            }
        }
        if !sleep_per_packet.is_zero() {
            std::thread::sleep(sleep_per_packet);
        }
    }

    // Successful walk -> delete consumed files. Errors are tolerated.
    for path in &files {
        let _ = fs::remove_file(path);
    }

    Ok(counts)
}

fn publish_sent<S: event::Subscriber>(subscriber: &S, id: &Id, peer: &SocketAddr) {
    use s2n_quic_core::time::{Clock as _, NoopClock};
    let timestamp = NoopClock.get_time().into_event();
    let publisher = event::EndpointPublisherSubscriber::new(
        event::builder::EndpointMeta { timestamp },
        None,
        subscriber,
    );
    publisher.on_unknown_path_secret_packet_sent(event::builder::UnknownPathSecretPacketSent {
        peer_address: SocketAddress::from(*peer).into_event(),
        credential_id: id.into_event(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::testing::Subscriber,
        packet::{secret_control as control, secret_control::TAG_LEN},
        path::secret::stateless_reset::Signer,
    };
    use std::{
        net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6, UdpSocket},
        sync::Arc,
    };
    use tempfile::TempDir;

    fn id_from(seed: u8) -> Id {
        let mut bytes = [0u8; 16];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = seed.wrapping_add(i as u8);
        }
        Id::from(bytes)
    }

    #[test]
    fn format_round_trip_v4() {
        let id = id_from(0x42);
        let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 5000));
        let mut buf = Vec::new();
        format::write_line(&mut buf, &id, &peer).unwrap();
        let s = std::str::from_utf8(&buf).unwrap();
        let (got_id, got_peer) = format::parse_line(s).unwrap();
        assert_eq!(*got_id, *id);
        assert_eq!(got_peer, peer);
    }

    #[test]
    fn format_round_trip_v6() {
        let id = id_from(0x99);
        let peer = SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1),
            8080,
            0,
            0,
        ));
        let mut buf = Vec::new();
        format::write_line(&mut buf, &id, &peer).unwrap();
        let s = std::str::from_utf8(&buf).unwrap();
        let (got_id, got_peer) = format::parse_line(s).unwrap();
        assert_eq!(*got_id, *id);
        assert_eq!(got_peer, peer);
    }

    #[test]
    fn format_rejects_bad_lines() {
        assert!(format::parse_line("").is_none());
        assert!(format::parse_line("garbage").is_none());
        // wrong hex length
        assert!(format::parse_line("ab 10.0.0.1:5000").is_none());
        // non-hex
        assert!(format::parse_line("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz 10.0.0.1:5000").is_none());
        // missing peer
        assert!(format::parse_line("00000000000000000000000000000000").is_none());
        // bad peer
        assert!(format::parse_line("00000000000000000000000000000000 not_a_peer").is_none());
    }

    #[test]
    fn writer_creates_dir_and_writes() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("nested/deeper");
        let writer = Writer::new(dir.clone()).unwrap();
        let id = id_from(1);
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        writer.write_snapshot([(id, peer)]).unwrap();
        assert!(dir.exists());
        let body = fs::read_to_string(writer.file_path()).unwrap();
        assert!(body.contains("127.0.0.1:1"));
    }

    #[test]
    fn writer_cleans_orphan_tmp_files() {
        let tmp = TempDir::new().unwrap();
        let orphan = tmp.path().join("peers-9999-0.txt.tmp");
        fs::write(&orphan, "stale").unwrap();
        // construct writer; orphan should be removed
        let _writer = Writer::new(tmp.path().to_path_buf()).unwrap();
        assert!(!orphan.exists(), "orphan tmp should be unlinked");
    }

    #[test]
    fn writer_atomic_replace() {
        let tmp = TempDir::new().unwrap();
        let writer = Writer::new(tmp.path().to_path_buf()).unwrap();
        let id1 = id_from(1);
        let id2 = id_from(2);
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        writer.write_snapshot([(id1, peer)]).unwrap();
        let after_first = fs::read_to_string(writer.file_path()).unwrap();
        assert_eq!(after_first.lines().count(), 1);
        // overwrite with different content
        writer.write_snapshot([(id2, peer)]).unwrap();
        let after_second = fs::read_to_string(writer.file_path()).unwrap();
        assert_eq!(after_second.lines().count(), 1);
        assert_ne!(after_first, after_second);
    }

    #[test]
    fn replay_empty_dir_returns_zero() {
        let tmp = TempDir::new().unwrap();
        let signer = Signer::new(b"test seed");
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            tmp.path(),
            1000,
            Duration::from_secs(1),
            &signer,
            &socket,
            &subscriber,
        )
        .unwrap();
        assert_eq!(counts.sent, 0);
        assert_eq!(counts.send_errors, 0);
        assert_eq!(counts.timed_out, 0);
    }

    #[test]
    fn replay_missing_dir_returns_zero() {
        let tmp = TempDir::new().unwrap();
        let nonexistent = tmp.path().join("does/not/exist");
        let signer = Signer::new(b"test seed");
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            &nonexistent,
            1000,
            Duration::from_secs(1),
            &signer,
            &socket,
            &subscriber,
        )
        .unwrap();
        assert_eq!(counts.sent, 0);
    }

    #[test]
    fn replay_sends_one_packet_per_unique_id() {
        let tmp = TempDir::new().unwrap();
        // Bind a localhost UDP receiver to capture packets.
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        let writer = Writer::new(tmp.path().to_path_buf()).unwrap();
        let id1 = id_from(1);
        let id2 = id_from(2);
        writer
            .write_snapshot([(id1, recv_addr), (id2, recv_addr)])
            .unwrap();

        let signer = Signer::new(b"test seed");
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            tmp.path(),
            1000,
            Duration::from_secs(2),
            &signer,
            &sender,
            &subscriber,
        )
        .unwrap();
        assert_eq!(counts.sent, 2);

        // Drain at least 2 packets.
        let mut buf = [0u8; 1500];
        let mut got = 0u32;
        for _ in 0..2 {
            if receiver.recv_from(&mut buf).is_ok() {
                got += 1;
            }
        }
        assert_eq!(got, 2);
    }

    #[test]
    fn replay_dedupes_by_id_last_wins() {
        let tmp = TempDir::new().unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();
        // write two files in the same dir; second has same id with different peer
        let first = Writer::new(tmp.path().to_path_buf()).unwrap();
        let id = id_from(7);
        let other_peer: SocketAddr = "127.0.0.1:9".parse().unwrap();
        first.write_snapshot([(id, other_peer)]).unwrap();

        // second writer (different file name due to counter)
        let second = Writer::new(tmp.path().to_path_buf()).unwrap();
        second.write_snapshot([(id, recv_addr)]).unwrap();

        let signer = Signer::new(b"seed");
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            tmp.path(),
            1000,
            Duration::from_secs(2),
            &signer,
            &sender,
            &subscriber,
        )
        .unwrap();
        assert_eq!(counts.sent, 1);
    }

    #[test]
    fn replay_deletes_consumed_files() {
        let tmp = TempDir::new().unwrap();
        let writer = Writer::new(tmp.path().to_path_buf()).unwrap();
        let id = id_from(1);
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        writer.write_snapshot([(id, peer)]).unwrap();
        let file = writer.file_path().to_path_buf();
        assert!(file.exists());

        let signer = Signer::new(b"seed");
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        replay(
            tmp.path(),
            1000,
            Duration::from_secs(2),
            &signer,
            &sender,
            &subscriber,
        )
        .unwrap();
        assert!(!file.exists(), "consumed file should be deleted");
    }

    #[test]
    fn cleaner_snapshot_then_replay_round_trip() {
        // End-to-end: build a Map with persistence, insert peers, manually
        // drive cleaner, drop the Map, then run replay() against the same dir
        // — packets should arrive at the same peer addresses.
        use crate::{
            event,
            path::secret::{map::Map, stateless_reset::Signer as MapSigner},
        };
        use s2n_quic_core::time::NoopClock;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        let signer_seed = b"shared seed for capture-replay";

        // Capture phase: insert via Map, force a cleaner pass.
        {
            let map = Map::new_with_persistence(
                MapSigner::new(signer_seed),
                100,
                false,
                NoopClock,
                event::testing::Subscriber::no_snapshot(),
                Some(dir.clone()),
            );
            map.test_insert(recv_addr);
            map.cleaner_run_for_test();
            // map drops here; persistence file is on disk.
        }

        // Replay phase: same signer seed.
        let signer = Signer::new(signer_seed);
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            &dir,
            1000,
            Duration::from_secs(2),
            &signer,
            &sender,
            &subscriber,
        )
        .unwrap();
        assert_eq!(counts.sent, 1, "exactly one packet should be replayed");

        let mut buf = [0u8; 1500];
        let (n, from) = receiver.recv_from(&mut buf).unwrap();
        assert_eq!(from.ip(), recv_addr.ip());
        assert!(n >= TAG_LEN);
    }

    #[test]
    fn replay_packet_authenticates_with_same_signer() {
        // The whole point: a server with the same signer can produce a packet
        // that a client (also using the same signer for its sender state)
        // would accept.
        let tmp = TempDir::new().unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        let writer = Writer::new(tmp.path().to_path_buf()).unwrap();
        let id = id_from(123);
        writer.write_snapshot([(id, recv_addr)]).unwrap();

        let signer = Signer::new(b"shared seed");
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let _ = replay(
            tmp.path(),
            1000,
            Duration::from_secs(1),
            &signer,
            &sender,
            &subscriber,
        )
        .unwrap();

        let mut buf = [0u8; 1500];
        let (n, _) = receiver.recv_from(&mut buf).unwrap();
        // The packet ends with the signer's tag for this id.
        let expected_tag = signer.sign(&id);
        assert!(n >= TAG_LEN);
        assert_eq!(&buf[n - TAG_LEN..n], &expected_tag[..]);
    }

    // ---- Server HI end-to-end loop closure (Rung 0 + Rung 1) ----
    //
    // The tests above prove a replayed UPS packet leaves the socket and carries
    // the correct signer tag *on the wire*. They stop there because an entry
    // seeded by `test_insert` has an all-zero stateless-reset tag (Entry::fake)
    // and therefore cannot authenticate a real UPS. The tests below close the
    // loop: they feed the replayed packet into a *receiving* Map whose entry was
    // seeded via `test_insert_pair` (so its `entry.sender().stateless_reset`
    // matches `signer.sign(&id)`), then assert the receiver authenticates,
    // schedules a re-handshake, and evicts. See test-plan.md in the Server HI
    // design folder for the full rationale.

    /// Rung 0: cross-restart determinism. The entire Server HI loop authenticates
    /// only because the capturing process and the replaying process derive the
    /// identical stateless-reset tag for a given credential_id from the same
    /// signer seed. If a future change sources the signer seed dynamically
    /// without persisting it across restarts, replayed packets stop
    /// authenticating and Server HI silently no-ops — while every
    /// transmission-only test above still passes. This pins the contract by
    /// exercising the round-trip: two independently-constructed signers built
    /// from the same seed must produce byte-identical tags.
    #[test]
    fn signer_seed_round_trip_is_deterministic() {
        use crate::path::secret::stateless_reset::Signer as MapSigner;

        let seed = b"capture and replay must agree";
        let capture_side = MapSigner::new(seed);
        let replay_side = MapSigner::new(seed);

        for raw in [0u8, 1, 42, 255] {
            let id = id_from(raw);
            assert_eq!(
                capture_side.sign(&id),
                replay_side.sign(&id),
                "tags diverged for id seeded from {raw}; cross-restart \
                 authentication would fail and Server HI would no-op",
            );
        }

        // A different seed MUST produce a different tag, otherwise the round-trip
        // assertion above is vacuous.
        let other = MapSigner::new(b"a different seed entirely");
        let id = id_from(7);
        assert_ne!(
            capture_side.sign(&id),
            other.sign(&id),
            "distinct seeds must produce distinct tags",
        );
    }

    /// Rung 1: full capture -> replay -> receive -> authenticate -> re-handshake
    /// + evict loop, in-process, across two real Maps and a real UDP socket.
    ///
    /// This is the test the feature's correctness claim rests on and that no
    /// prior test covered: it is the first to feed a *replayed* packet (off the
    /// wire, produced by `replay()`) into a receiving Map and assert the
    /// receiver acts on it.
    #[test]
    fn replay_drives_receiver_rehandshake_and_eviction() {
        use crate::{
            event,
            path::secret::{map::Map, stateless_reset::Signer as MapSigner},
            psk::io::HandshakeReason,
        };
        use s2n_quic_core::time::NoopClock;
        use std::sync::Mutex;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();

        // The receiver's UDP socket is the "client handshake address" that the
        // captured entry points at; replay() will send the UPS here.
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        let seed = b"rung1 shared capture-replay seed";

        // Receiving ("client") Map, built with the shared seed and evict-on-UPS
        // enabled so we can observe eviction. We register a request_handshake
        // callback that records the peer it was asked to re-handshake with.
        let client = Map::new_with_persistence(
            MapSigner::new(seed),
            10,
            /* should_evict_on_unknown_path_secret */ true,
            NoopClock,
            event::testing::Subscriber::no_snapshot(),
            None,
        );
        let rehandshakes: Arc<Mutex<Vec<SocketAddr>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let rehandshakes = rehandshakes.clone();
            client.register_request_handshake(Box::new(move |peer, reason| {
                assert!(matches!(reason, HandshakeReason::Remote));
                rehandshakes.lock().unwrap().push(peer);
                None
            }));
        }

        // Capturing ("server") Map, built with the SAME seed and persistence
        // configured. test_insert_pair installs a matched pair (see map.rs):
        //   - the SERVER entry is keyed at the client's address (recv_addr) and
        //     is what gets persisted + replayed-to;
        //   - the CLIENT entry is keyed at the server's address
        //     (server_local_addr) with stateless_reset = server_signer.sign(&id),
        //     exactly what replay() (same seed) will sign.
        // The receive handler looks the entry up by credential_id, not address,
        // so authentication works regardless of which socket delivered the UPS.
        let server_local_addr: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let credential_id;
        {
            let server = Map::new_with_persistence(
                MapSigner::new(seed),
                10,
                false,
                NoopClock,
                event::testing::Subscriber::no_snapshot(),
                Some(dir.clone()),
            );
            credential_id =
                server.test_insert_pair(server_local_addr, None, &client, recv_addr, None);

            // Snapshot the server's live entries to disk, then drop the server —
            // this is the "process exits" boundary.
            server.cleaner_run_for_test();
        }

        // Sanity: the client holds its entry (keyed at the server address) before
        // the UPS arrives.
        assert!(
            client.contains(&server_local_addr),
            "client should hold the entry prior to UPS",
        );

        // Replay phase: a fresh signer from the same seed, a transient socket.
        let signer = Signer::new(seed);
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            &dir,
            1000,
            Duration::from_secs(2),
            &signer,
            &sender,
            &subscriber,
        )
        .unwrap();
        assert_eq!(counts.sent, 1, "exactly one UPS should be replayed");

        // Receive the replayed packet off the wire and feed it into the client
        // Map exactly as the production receive path does.
        let mut buf = [0u8; 1500];
        let (n, from) = receiver.recv_from(&mut buf).unwrap();
        let (packet, _) =
            control::Packet::decode(s2n_codec::DecoderBufferMut::new(&mut buf[..n])).unwrap();
        client.handle_control_packet(&packet, &from);

        // The receiver authenticated the UPS and acted on it:
        // 1. a re-handshake was scheduled with the captured peer. request_handshake
        //    is invoked with entry.peer(); for the client entry that is the
        //    server's address.
        let scheduled = rehandshakes.lock().unwrap();
        assert_eq!(
            scheduled.len(),
            1,
            "exactly one re-handshake should be scheduled",
        );
        assert_eq!(
            scheduled[0], server_local_addr,
            "re-handshake should target the server the client had handshook with",
        );
        drop(scheduled);
        // 2. the entry was evicted (should_evict_on_unknown_path_secret = true).
        assert!(
            !client.contains(&server_local_addr),
            "entry should be evicted after an authenticated UPS",
        );
        // Mention the credential so the binding is obviously load-bearing.
        let _ = credential_id;
    }

    /// Rung 1 negative: a receiver built with a DIFFERENT seed must reject the
    /// replayed UPS — no re-handshake, no eviction. This proves the seed is
    /// load-bearing (authentication is real, not incidental).
    #[test]
    fn replay_with_mismatched_seed_is_rejected() {
        use crate::{
            event,
            path::secret::{map::Map, stateless_reset::Signer as MapSigner},
            psk::io::HandshakeReason,
        };
        use s2n_quic_core::time::NoopClock;
        use std::sync::Mutex;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();

        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        // Client uses the "real" seed; capture/replay will use a different one.
        let client_seed = b"client genuine seed value here!!";
        let client = Map::new_with_persistence(
            MapSigner::new(client_seed),
            10,
            true,
            NoopClock,
            event::testing::Subscriber::no_snapshot(),
            None,
        );
        let rehandshakes: Arc<Mutex<Vec<SocketAddr>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let rehandshakes = rehandshakes.clone();
            client.register_request_handshake(Box::new(move |peer, _: HandshakeReason| {
                rehandshakes.lock().unwrap().push(peer);
                None
            }));
        }

        let server_local_addr: SocketAddr = "127.0.0.1:9".parse().unwrap();
        {
            // Server (capture) shares the client's seed so test_insert_pair builds
            // a coherent matched entry on the client — but REPLAY below uses a
            // different signer, modelling a post-restart seed that drifted.
            let server = Map::new_with_persistence(
                MapSigner::new(client_seed),
                10,
                false,
                NoopClock,
                event::testing::Subscriber::no_snapshot(),
                Some(dir.clone()),
            );
            server.test_insert_pair(server_local_addr, None, &client, recv_addr, None);
            server.cleaner_run_for_test();
        }

        assert!(client.contains(&server_local_addr));

        // Replay with a MISMATCHED seed: the tag will not match the client
        // entry's stateless_reset, so authenticate() fails.
        let wrong_signer = Signer::new(b"drifted seed after restart bad!!");
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let subscriber = Subscriber::no_snapshot();
        let counts = replay(
            &dir,
            1000,
            Duration::from_secs(2),
            &wrong_signer,
            &sender,
            &subscriber,
        )
        .unwrap();
        assert_eq!(
            counts.sent, 1,
            "a packet is still sent; it just won't authenticate"
        );

        let mut buf = [0u8; 1500];
        let (n, from) = receiver.recv_from(&mut buf).unwrap();
        let (packet, _) =
            control::Packet::decode(s2n_codec::DecoderBufferMut::new(&mut buf[..n])).unwrap();
        client.handle_control_packet(&packet, &from);

        // Rejected: no re-handshake scheduled, entry NOT evicted.
        assert_eq!(
            rehandshakes.lock().unwrap().len(),
            0,
            "a mismatched-seed UPS must not schedule a re-handshake",
        );
        assert!(
            client.contains(&server_local_addr),
            "a mismatched-seed UPS must not evict the entry",
        );
    }
}

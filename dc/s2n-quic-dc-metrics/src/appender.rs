// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs::OpenOptions,
    io::{BufWriter, Result, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, Timelike, Utc};

trait Filesystem: Send + Sync + 'static {
    fn create_dir(&self, path: &Path) -> Result<()>;
    fn open(&self, path: &Path) -> Result<Box<dyn Write + Send + Sync + 'static>>;
    fn list_files(&self, dir: &Path) -> Vec<PathBuf>;
    fn remove(&self, path: &Path) -> Result<()>;
}

struct OsFilesystem;

impl Filesystem for OsFilesystem {
    fn create_dir(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Write + Send + Sync + 'static>> {
        let f = OpenOptions::new()
            .read(false)
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Box::new(f))
    }

    fn list_files(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
            .map(|e| e.path())
            .collect()
    }

    fn remove(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path)
    }
}

struct NoopFilesystem;

impl Filesystem for NoopFilesystem {
    fn create_dir(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn open(&self, _path: &Path) -> Result<Box<dyn Write + Send + Sync + 'static>> {
        Ok(Box::new(std::io::sink()))
    }

    fn list_files(&self, _dir: &Path) -> Vec<PathBuf> {
        Vec::new()
    }

    fn remove(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
}

pub struct Builder {
    fs: Box<dyn Filesystem>,
    base_path: PathBuf,
    filename_prefix: String,
    log_rotation: std::time::Duration,
    max_files: usize,
}

impl Builder {
    pub fn new(
        base_path: PathBuf,
        filename_prefix: String,
        log_rotation: std::time::Duration,
    ) -> Self {
        Self {
            fs: Box::new(OsFilesystem),
            base_path,
            filename_prefix,
            log_rotation,
            max_files: 0,
        }
    }

    pub fn max_files(&mut self, max_files: usize) -> &mut Self {
        self.max_files = max_files;
        self
    }

    pub fn build(self) -> Result<MetricsWriter> {
        self.fs.create_dir(&self.base_path)?;

        Ok(MetricsWriter {
            fs: self.fs,
            base_path: self.base_path,
            filename_prefix: self.filename_prefix,
            active: None,
            log_rotation: Duration::from_std(self.log_rotation)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            time_source: Box::new(Utc::now),
            max_files: self.max_files,
        })
    }
}

/// Rolling service log writer. Automatically opens new service log files each period (supports
/// hourly and sub-hourly rotation).
pub struct MetricsWriter {
    fs: Box<dyn Filesystem>,
    base_path: PathBuf,
    filename_prefix: String,
    active: Option<ActiveWriter>,
    time_source: Box<dyn Fn() -> DateTime<Utc> + Send + Sync + 'static>,
    log_rotation: Duration,
    max_files: usize,
}

impl std::fmt::Debug for MetricsWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsWriter")
            .field("base_path", &self.base_path)
            .field("filename_prefix", &self.filename_prefix)
            .field("active", &self.active)
            .field("max_files", &self.max_files)
            .finish()
    }
}

impl MetricsWriter {
    pub fn builder(
        base_path: PathBuf,
        filename_prefix: String,
        log_rotation: std::time::Duration,
    ) -> Builder {
        Builder::new(base_path, filename_prefix, log_rotation)
    }

    /// Creates a new metrics writer.
    ///
    /// Arguments:
    ///
    /// * `base_path` - The directory to write service logs to
    /// * `filename_prefix` - The prefix for the filenames of the service logs (usually `service_log`)
    pub fn new(
        base_path: PathBuf,
        filename_prefix: String,
        log_rotation: std::time::Duration,
    ) -> Result<Self> {
        Builder::new(base_path, filename_prefix, log_rotation).build()
    }

    pub fn noop() -> MetricsWriter {
        Self {
            fs: Box::new(NoopFilesystem),
            base_path: PathBuf::new(),
            filename_prefix: String::new(),
            active: None,
            log_rotation: Duration::try_minutes(60).unwrap(),
            time_source: Box::new(Utc::now),
            max_files: 0,
        }
    }
}

/// Information about a single opened file
struct ActiveWriter {
    filename: PathBuf,
    valid_starting: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    writer: BufWriter<Box<dyn Write + Send + Sync + 'static>>,
}

impl std::fmt::Debug for ActiveWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveWriter")
            .field("filename", &self.filename)
            .field("valid_until", &self.valid_until)
            .finish()
    }
}

struct FlushOnDrop<T: Write>(T);

impl<T: Write> Write for FlushOnDrop<T> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        self.0.flush()
    }
}

impl<T: Write> Drop for FlushOnDrop<T> {
    fn drop(&mut self) {
        let _ = self.0.flush();
    }
}

impl MetricsWriter {
    /// Obtains an object which will write to the currently opened service log
    /// file. May open a new service log file if necessary. May delete old
    /// service log files if the configured max file count has been exceeded.
    ///
    /// The returned object will flush any unwritten data when dropped.
    pub fn writer(&'_ mut self) -> std::io::Result<impl std::io::Write + '_> {
        let now = (self.time_source)();

        if let Some(active) = self.active.as_mut() {
            if now < active.valid_starting || now >= active.valid_until {
                self.active = None;
            }
        }

        if self.active.is_none() {
            self.active = Some(self.open_writer(now)?);
        }

        Ok(FlushOnDrop(&mut self.active.as_mut().unwrap().writer))
    }

    fn cleanup_old_files(&self) {
        if self.max_files == 0 {
            return;
        }

        let prefix = self.base_path.join(format!("{}.", self.filename_prefix));
        let prefix = prefix.as_os_str().as_encoded_bytes();
        let mut files: Vec<_> = self
            .fs
            .list_files(&self.base_path)
            .into_iter()
            .filter(|path| path.as_os_str().as_encoded_bytes().starts_with(prefix))
            .collect();

        if files.len() <= self.max_files {
            return;
        }

        // Each file includes a timestamp. So, when sorted ascending by path, the first files are
        // the oldest, and should be deleted first.
        files.sort();

        for path in &files[..files.len() - self.max_files] {
            let _ = self.fs.remove(path);
        }
    }

    fn open_writer(&self, now: DateTime<Utc>) -> std::io::Result<ActiveWriter> {
        let mut filename = format!("{}.{}", &self.filename_prefix, now.format("%Y-%m-%d-%H"));
        let mut start_time = now.date_naive().and_hms_opt(now.hour(), 0, 0).unwrap();
        if self.log_rotation.num_minutes() < 60 {
            filename = format!("{}.{}", &self.filename_prefix, now.format("%Y-%m-%d-%H-%M"));
            start_time = now
                .date_naive()
                .and_hms_opt(now.hour(), now.minute(), 0)
                .unwrap();
        }

        let mut path = self.base_path.clone();
        path.push(filename);

        let file = self.fs.open(&path)?;
        self.cleanup_old_files();
        let deadline = start_time.checked_add_signed(self.log_rotation).unwrap();

        Ok(ActiveWriter {
            filename: path,
            valid_starting: DateTime::from_naive_utc_and_offset(start_time, Utc),
            valid_until: DateTime::from_naive_utc_and_offset(deadline, Utc),
            writer: BufWriter::new(file),
        })
    }
}

#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;

    use std::sync::Mutex;

    // (flushed, buffer)
    type MutexBuf = Arc<Mutex<(bool, Vec<u8>)>>;

    struct MutexStringWriter(MutexBuf);

    impl std::io::Write for MutexStringWriter {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            let mut lock = self.0.lock().unwrap();

            lock.1.extend_from_slice(buf);

            Ok(buf.len())
        }

        fn flush(&mut self) -> Result<()> {
            self.0.lock().unwrap().0 = true;
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct MockFilesystem(Arc<Mutex<BTreeMap<PathBuf, MutexBuf>>>);

    impl MockFilesystem {
        fn new() -> Self {
            Self::default()
        }

        #[track_caller]
        fn assert_opened(&self, path: impl AsRef<Path>) -> MutexBuf {
            let key = path.as_ref();
            self.0
                .lock()
                .unwrap()
                .remove(key)
                .unwrap_or_else(|| panic!("File not opened: {}", key.display()))
                .clone()
        }

        fn assert_not_opened(&self) {
            assert!(self.0.lock().unwrap().is_empty());
        }
    }

    impl Filesystem for MockFilesystem {
        fn create_dir(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        fn open(&self, path: &Path) -> Result<Box<dyn Write + Send + Sync + 'static>> {
            let buf = MutexBuf::default();
            self.0
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), buf.clone());
            Ok(Box::new(MutexStringWriter(buf)))
        }

        fn list_files(&self, dir: &Path) -> Vec<PathBuf> {
            self.0
                .lock()
                .unwrap()
                .keys()
                .filter(|p| p.parent() == Some(dir))
                .cloned()
                .collect()
        }

        fn remove(&self, path: &Path) -> Result<()> {
            self.0.lock().unwrap().remove(path);
            Ok(())
        }
    }

    fn test_writer(
        fs: MockFilesystem,
        now: Arc<Mutex<DateTime<Utc>>>,
        log_rotation: Duration,
        max_files: usize,
    ) -> MetricsWriter {
        MetricsWriter {
            base_path: "/foo/bar".into(),
            filename_prefix: "baz".into(),
            active: None,
            time_source: Box::new(move || *now.lock().unwrap()),
            fs: Box::new(fs),
            log_rotation,
            max_files,
        }
    }

    #[test]
    fn test_os_list_files() {
        let fs = OsFilesystem;
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let files = fs.list_files(dir);

        // Cargo.toml should be listed
        assert!(files.iter().any(|p| p.file_name().unwrap() == "Cargo.toml"));

        // No directories should be listed (e.g. src/)
        for path in &files {
            assert!(path.is_file(), "{} is not a file", path.display());
        }
    }

    #[test]
    fn test_reuse() {
        let now = Arc::new(Mutex::new(
            "2020-01-01T00:00:00+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        let mut writer = test_writer(files.clone(), now, Duration::minutes(60), 0);

        write!(&mut writer.writer().unwrap(), "foobar").unwrap();

        let buf = files.assert_opened("/foo/bar/baz.2020-01-01-00");
        assert_eq!(
            (true, Vec::from(&b"foobar"[..])),
            buf.lock().unwrap().clone()
        );
        *buf.lock().unwrap() = (false, vec![]);

        write!(&mut writer.writer().unwrap(), "quux").unwrap();
        assert_eq!((true, Vec::from(&b"quux"[..])), buf.lock().unwrap().clone());
        *buf.lock().unwrap() = (false, vec![]);
    }

    #[test]
    fn test_rotation() {
        let now = Arc::new(Mutex::new(
            "2020-01-23T12:34:56+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        let mut writer = test_writer(files.clone(), now.clone(), Duration::minutes(60), 0);

        write!(&mut writer.writer().unwrap(), "foobar").unwrap();

        let buf = files.assert_opened("/foo/bar/baz.2020-01-23-12");
        assert_eq!(
            (true, Vec::from(&b"foobar"[..])),
            buf.lock().unwrap().clone()
        );
        *buf.lock().unwrap() = (false, vec![]);

        *now.lock().unwrap() = "2020-01-23T12:59:59+00:00"
            .parse::<DateTime<Utc>>()
            .unwrap();

        write!(&mut writer.writer().unwrap(), "foobar").unwrap();

        files.assert_not_opened();
        assert_eq!(
            (true, Vec::from(&b"foobar"[..])),
            buf.lock().unwrap().clone()
        );
        *buf.lock().unwrap() = (false, vec![]);

        *now.lock().unwrap() = "2020-01-23T13:00:00+00:00"
            .parse::<DateTime<Utc>>()
            .unwrap();
        write!(&mut writer.writer().unwrap(), "quux").unwrap();
        let buf = files.assert_opened("/foo/bar/baz.2020-01-23-13");
        assert_eq!((true, Vec::from(&b"quux"[..])), buf.lock().unwrap().clone());
    }

    #[test]
    fn test_configured_rotation() {
        let now = Arc::new(Mutex::new(
            "2020-01-23T12:34:56+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        let mut writer = test_writer(files.clone(), now.clone(), Duration::minutes(5), 0);

        write!(&mut writer.writer().unwrap(), "foobar").unwrap();

        let buf = files.assert_opened("/foo/bar/baz.2020-01-23-12-34");
        assert_eq!(
            (true, Vec::from(&b"foobar"[..])),
            buf.lock().unwrap().clone()
        );
        *buf.lock().unwrap() = (false, vec![]);

        *now.lock().unwrap() = "2020-01-23T12:35:59+00:00"
            .parse::<DateTime<Utc>>()
            .unwrap();

        write!(&mut writer.writer().unwrap(), "foobar").unwrap();

        files.assert_not_opened();
        assert_eq!(
            (true, Vec::from(&b"foobar"[..])),
            buf.lock().unwrap().clone()
        );
        *buf.lock().unwrap() = (false, vec![]);

        *now.lock().unwrap() = "2020-01-23T12:39:00+00:00"
            .parse::<DateTime<Utc>>()
            .unwrap();
        write!(&mut writer.writer().unwrap(), "quux").unwrap();
        let buf = files.assert_opened("/foo/bar/baz.2020-01-23-12-39");
        assert_eq!((true, Vec::from(&b"quux"[..])), buf.lock().unwrap().clone());
    }

    #[test]
    fn test_time_rollback() {
        let now = Arc::new(Mutex::new(
            "2020-01-23T12:34:56+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        let mut writer = test_writer(files.clone(), now.clone(), Duration::minutes(60), 0);

        write!(&mut writer.writer().unwrap(), "foobar").unwrap();

        let buf = files.assert_opened("/foo/bar/baz.2020-01-23-12");
        assert_eq!(
            (true, Vec::from(&b"foobar"[..])),
            buf.lock().unwrap().clone()
        );

        *now.lock().unwrap() = "2020-01-01T13:00:00+00:00"
            .parse::<DateTime<Utc>>()
            .unwrap();
        write!(&mut writer.writer().unwrap(), "quux").unwrap();
        let buf = files.assert_opened("/foo/bar/baz.2020-01-01-13");
        assert_eq!((true, Vec::from(&b"quux"[..])), buf.lock().unwrap().clone());
    }

    #[test]
    fn flush_on_drop() {
        let now = Arc::new(Mutex::new(
            "2020-01-01T00:00:00+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        let mut writer = test_writer(files.clone(), now, Duration::minutes(60), 0);

        let mut handle = writer.writer().unwrap();
        let buf = files.assert_opened("/foo/bar/baz.2020-01-01-00");
        assert_eq!((false, vec![]), buf.lock().unwrap().clone());

        write!(&mut handle, "x").unwrap();
        assert!(matches!(&*buf.lock().unwrap(), (false, _)));

        drop(handle);
        assert_eq!((true, vec![b'x']), buf.lock().unwrap().clone());
    }

    #[test]
    fn test_cleanup_deletes_oldest_files() {
        let now = Arc::new(Mutex::new(
            "2020-01-01T05:00:00+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        for name in [
            "baz.2020-01-01-00",
            "baz.2020-01-01-01",
            "baz.2020-01-01-02",
            "baz.2020-01-01-03",
            "baz.2020-01-01-04",
            "other_file.txt",
        ] {
            let _ = files.open(Path::new("/foo/bar").join(name).as_path());
        }
        let mut writer = test_writer(files.clone(), now, Duration::minutes(60), 3);

        write!(&mut writer.writer().unwrap(), "new data").unwrap();

        assert_eq!(
            files.list_files(Path::new("/foo/bar")),
            vec![
                PathBuf::from("/foo/bar/baz.2020-01-01-03"),
                PathBuf::from("/foo/bar/baz.2020-01-01-04"),
                PathBuf::from("/foo/bar/baz.2020-01-01-05"),
                PathBuf::from("/foo/bar/other_file.txt"),
            ]
        );
    }

    #[test]
    fn test_cleanup_disabled_when_max_files_is_zero() {
        let now = Arc::new(Mutex::new(
            "2020-01-01T05:00:00+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        for hour in 0..5 {
            let _ = files.open(&Path::new("/foo/bar").join(format!("baz.2020-01-01-{hour:02}")));
        }
        let mut writer = test_writer(files.clone(), now, Duration::minutes(60), 0);

        write!(&mut writer.writer().unwrap(), "new data").unwrap();

        assert_eq!(files.list_files(Path::new("/foo/bar")).len(), 6);
    }

    #[test]
    fn test_cleanup_across_multiple_rotations() {
        let now = Arc::new(Mutex::new(
            "2020-01-01T00:00:00+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let files = MockFilesystem::default();
        let mut writer = test_writer(files.clone(), now.clone(), Duration::minutes(60), 2);

        for hour in 0..4u32 {
            *now.lock().unwrap() = format!("2020-01-01T{hour:02}:00:00+00:00")
                .parse::<DateTime<Utc>>()
                .unwrap();
            write!(&mut writer.writer().unwrap(), "hour {hour}").unwrap();
        }

        assert_eq!(
            files.list_files(Path::new("/foo/bar")),
            vec![
                PathBuf::from("/foo/bar/baz.2020-01-01-02"),
                PathBuf::from("/foo/bar/baz.2020-01-01-03"),
            ]
        );
    }
}

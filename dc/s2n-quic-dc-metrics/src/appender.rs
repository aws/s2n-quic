use std::{
    fs::OpenOptions,
    io::{BufWriter, Result, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, Timelike, Utc};

/// Rolling service log writer. Automatically opens new service log files each period (supports
/// hourly and sub-hourly rotation).
pub struct MetricsWriter {
    base_path: PathBuf,
    filename_prefix: String,
    active: Option<ActiveWriter>,
    time_source: Box<dyn Fn() -> DateTime<Utc> + Send + Sync + 'static>,
    #[allow(clippy::type_complexity)]
    file_factory: Box<
        dyn Fn(&Path) -> Result<Box<dyn Write + Send + Sync + 'static>> + Send + Sync + 'static,
    >,
    log_rotation: Duration,
}

impl std::fmt::Debug for MetricsWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsWriter")
            .field("base_path", &self.base_path)
            .field("filename_prefix", &self.filename_prefix)
            .field("active", &self.active)
            .finish()
    }
}

impl MetricsWriter {
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
        std::fs::create_dir_all(&base_path)?;

        Ok(Self {
            base_path,
            filename_prefix,
            active: None,
            log_rotation: Duration::from_std(log_rotation)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            time_source: Box::new(Utc::now),
            file_factory: Box::new(|path| {
                let f = OpenOptions::new()
                    .read(false)
                    .create(true)
                    .append(true)
                    .open(path)?;

                Ok(Box::new(f))
            }),
        })
    }

    pub fn noop() -> MetricsWriter {
        Self {
            base_path: PathBuf::new(),
            filename_prefix: String::new(),
            active: None,
            log_rotation: Duration::try_minutes(60).unwrap(),
            time_source: Box::new(Utc::now),
            file_factory: Box::new(|_| Ok(Box::new(std::io::sink()))),
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
    /// file. May open a new service log file if necessary.
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

        let file = (self.file_factory)(&path)?;
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
    use std::{collections::VecDeque, sync::Arc};

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

    #[derive(Default)]
    struct MockFilesystem(Arc<Mutex<VecDeque<(PathBuf, MutexBuf)>>>);

    impl MockFilesystem {
        fn new() -> Self {
            Self::default()
        }

        #[allow(clippy::unnecessary_wraps)] // match non-test api
        fn open(
            &self,
            path: impl Into<PathBuf>,
        ) -> std::io::Result<Box<dyn Write + Send + Sync + 'static>> {
            let mut lock = self.0.lock().unwrap();

            let buf = MutexBuf::default();
            lock.push_back((path.into(), buf.clone()));

            Ok(Box::new(MutexStringWriter(buf)))
        }

        #[track_caller]
        fn assert_opened(&self, path: impl AsRef<Path>) -> MutexBuf {
            let mut lock = self.0.lock().unwrap();

            let (last_path, last_buf) = lock.pop_front().expect("No files opened");

            assert_eq!(&last_path, path.as_ref());

            last_buf
        }

        fn assert_not_opened(&self) {
            assert!(self.0.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn test_reuse() {
        let now = Arc::new(Mutex::new(
            "2020-01-01T00:00:00+00:00"
                .parse::<DateTime<Utc>>()
                .unwrap(),
        ));

        let time_source = { Box::new(move || *now.lock().unwrap()) };

        let files = Arc::new(MockFilesystem::new());

        let mut writer = MetricsWriter {
            base_path: "/foo/bar".into(),
            filename_prefix: "baz".into(),
            active: None,
            time_source,
            file_factory: {
                let files = files.clone();
                Box::new(move |path| files.open(path))
            },
            log_rotation: Duration::minutes(60),
        };

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

        let time_source = {
            let now = now.clone();
            Box::new(move || *now.lock().unwrap())
        };

        let files = Arc::new(MockFilesystem::new());

        let mut writer = MetricsWriter {
            base_path: "/foo/bar".into(),
            filename_prefix: "baz".into(),
            active: None,
            time_source,
            file_factory: {
                let files = files.clone();
                Box::new(move |path| files.open(path))
            },
            log_rotation: Duration::minutes(60),
        };

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

        let time_source = {
            let now = now.clone();
            Box::new(move || *now.lock().unwrap())
        };

        let files = Arc::new(MockFilesystem::new());

        let mut writer = MetricsWriter {
            base_path: "/foo/bar".into(),
            filename_prefix: "baz".into(),
            active: None,
            time_source,
            file_factory: {
                let files = files.clone();
                Box::new(move |path| files.open(path))
            },
            log_rotation: Duration::minutes(5),
        };

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

        let time_source = {
            let now = now.clone();
            Box::new(move || *now.lock().unwrap())
        };

        let files = Arc::new(MockFilesystem::new());

        let mut writer = MetricsWriter {
            base_path: "/foo/bar".into(),
            filename_prefix: "baz".into(),
            active: None,
            time_source,
            file_factory: {
                let files = files.clone();
                Box::new(move |path| files.open(path))
            },
            log_rotation: Duration::minutes(60),
        };

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

        let time_source = { Box::new(move || *now.lock().unwrap()) };

        let files = Arc::new(MockFilesystem::new());

        let mut writer = MetricsWriter {
            base_path: "/foo/bar".into(),
            filename_prefix: "baz".into(),
            active: None,
            time_source,
            file_factory: {
                let files = files.clone();
                Box::new(move |path| files.open(path))
            },
            log_rotation: Duration::minutes(60),
        };

        let mut handle = writer.writer().unwrap();
        let buf = files.assert_opened("/foo/bar/baz.2020-01-01-00");
        assert_eq!((false, vec![]), buf.lock().unwrap().clone());

        write!(&mut handle, "x").unwrap();
        assert!(matches!(&*buf.lock().unwrap(), (false, _)));

        drop(handle);
        assert_eq!((true, vec![b'x']), buf.lock().unwrap().clone());
    }
}

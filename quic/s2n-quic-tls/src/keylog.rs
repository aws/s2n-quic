// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use amzn_s2n_tls::raw::*;
use libc::{c_int, c_void};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    sync::{Arc, Mutex},
};

pub type KeyLogHandle = Arc<KeyLog>;

pub struct KeyLog(Mutex<BufWriter<File>>);

impl KeyLog {
    pub fn try_open() -> Option<KeyLogHandle> {
        let path = std::env::var("SSLKEYLOGFILE").ok()?;
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .ok()?;
        let file = BufWriter::new(file);
        let file = Mutex::new(file);
        let keylog = Self(file);
        let keylog = Arc::new(keylog);
        Some(keylog)
    }

    pub unsafe extern "C" fn callback(
        ctx: *mut c_void,
        _conn: *mut s2n_connection,
        logline: *mut u8,
        len: usize,
    ) -> c_int {
        let handle = &mut *(ctx as *mut Self);
        let logline = core::slice::from_raw_parts(logline, len);

        // ignore any errors
        let _ = handle.on_logline(logline);

        0
    }

    fn on_logline(&mut self, logline: &[u8]) -> Option<()> {
        let mut file = self.0.lock().ok()?;
        file.write_all(logline).ok()?;
        file.write_all(b"\n").ok()?;

        // ensure keys are immediately written so tools can use them
        file.flush().ok()?;

        Some(())
    }
}

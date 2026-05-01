// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_dc::busy_poll;
use std::sync::OnceLock;

pub struct BusyPoll {
    inner: OnceLock<busy_poll::Pool>,
    workers: usize,
}

impl BusyPoll {
    const fn new(workers: usize) -> Self {
        Self {
            inner: OnceLock::new(),
            workers,
        }
    }

    fn init(&self) -> busy_poll::Pool {
        let mut handles = Vec::with_capacity(self.workers);

        tracing::info!(workers = self.workers, "Initializing busy poll workers");

        for idx in 0..self.workers {
            let (handle, runner) = busy_poll::Handle::new();

            std::thread::Builder::new()
                .name(format!("wheel-demo:busy_poll:{}", idx))
                .spawn(move || {
                    Self::configure_thread();
                    runner.run()
                })
                .unwrap();

            handles.push(handle);
        }
        handles.into()
    }

    #[cfg(target_os = "linux")]
    fn configure_thread() {
        unsafe {
            // Set high nice priority (-20 is highest priority for non-RT threads)
            let result = libc::setpriority(libc::PRIO_PROCESS, 0, -20);
            if result != 0 {
                tracing::warn!(
                    "Failed to set high nice priority for busy poll thread: {}",
                    std::io::Error::last_os_error()
                );
            } else {
                tracing::debug!("Set busy poll thread priority to -20");
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn configure_thread() {
        // No-op on non-Linux platforms
    }
}

impl core::ops::Deref for BusyPoll {
    type Target = busy_poll::Pool;

    fn deref(&self) -> &Self::Target {
        self.inner.get_or_init(|| self.init())
    }
}

// Single busy poll pool for the demo (client uses it for sends, server for receives)
pub static BUSY_POLL: BusyPoll = BusyPoll::new(8);

/// Returns a clone of the busy poll pool
pub fn pool() -> busy_poll::Pool {
    BUSY_POLL.clone()
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Signal-based thread dump for busy-poll workers.
//!
//! Registers a SIGUSR1 handler that captures a backtrace when delivered.
//! The watchdog sends SIGUSR1 to specific worker threads via tgkill to
//! get their stack traces without needing ptrace or an external debugger.

use std::{
    backtrace::Backtrace,
    sync::{Mutex, Once},
    time::{Duration, Instant},
};

static HANDLER_INIT: Once = Once::new();
static RESULT: Mutex<Option<Backtrace>> = Mutex::new(None);

/// Installs the SIGUSR1 handler (idempotent).
pub fn install_handler() {
    HANDLER_INIT.call_once(|| unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as usize;
        sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGUSR1, &sa, std::ptr::null_mut());
    });
}

/// Sends SIGUSR1 to the given thread and waits up to `timeout` for the backtrace.
///
/// Returns `None` if the thread didn't respond in time (e.g. deadlocked in the allocator).
pub fn dump(tid: i32, timeout: Duration) -> Option<Backtrace> {
    if tid <= 0 {
        return None;
    }

    // Clear any stale result
    if let Ok(mut guard) = RESULT.lock() {
        *guard = None;
    }

    unsafe {
        libc::syscall(libc::SYS_tgkill, libc::getpid(), tid, libc::SIGUSR1);
    }

    let deadline = Instant::now() + timeout;
    loop {
        std::thread::sleep(Duration::from_millis(100));

        if let Ok(mut guard) = RESULT.lock() {
            if guard.is_some() {
                return guard.take();
            }
        }

        if Instant::now() >= deadline {
            return None;
        }
    }
}

extern "C" fn signal_handler(
    _sig: libc::c_int,
    _info: *mut libc::siginfo_t,
    _ctx: *mut libc::c_void,
) {
    let bt = Backtrace::force_capture();
    if let Ok(mut guard) = RESULT.try_lock() {
        *guard = Some(bt);
    }
}

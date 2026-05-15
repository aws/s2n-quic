// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_dc::busy_poll;

pub fn create_pool(workers: usize) -> busy_poll::Pool {
    tracing::info!(workers, "Initializing busy poll workers");

    let mut handles = Vec::with_capacity(workers);

    for idx in 0..workers {
        let (handle, runner) = busy_poll::Handle::new();

        std::thread::Builder::new()
            .name(format!("dcquic:busy_poll:{}", idx))
            .spawn(move || {
                configure_thread();
                runner.run()
            })
            .unwrap();

        handles.push(handle);
    }

    let pool: busy_poll::Pool = handles.into();
    pool.spawn_watchdog(std::time::Duration::from_secs(5));
    pool
}

#[cfg(target_os = "linux")]
fn configure_thread() {
    unsafe {
        let result = libc::setpriority(libc::PRIO_PROCESS, 0, -20);
        if result != 0 {
            tracing::warn!(
                "Failed to set high nice priority for busy poll thread: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_thread() {}

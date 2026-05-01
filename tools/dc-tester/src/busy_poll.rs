use s2n_quic_dc::busy_poll;
use std::sync::OnceLock;

static ALLOWED_CPUS: OnceLock<Vec<usize>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
enum PoolType {
    Send,
    Recv,
}

impl PoolType {
    const fn name(&self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Recv => "recv",
        }
    }

    const fn base_offset(&self) -> usize {
        match self {
            Self::Send => 0,
            Self::Recv => 1,
        }
    }
}

pub struct BusyPoll {
    inner: OnceLock<busy_poll::Pool>,
    pool_type: PoolType,
}

impl BusyPoll {
    const fn new(pool_type: PoolType) -> Self {
        Self {
            inner: OnceLock::new(),
            pool_type,
        }
    }

    fn init(&self) -> busy_poll::Pool {
        // Set rtprio rlimit so threads can use RT scheduling
        // This requires CAP_SYS_NICE capability on the binary
        Self::set_rtprio_limit();

        let requested_workers = 2;

        // Query the actual allowed CPU set for this process (cached)
        let allowed_cpus = ALLOWED_CPUS.get_or_init(Self::get_allowed_cpus);
        let available_cpus = allowed_cpus.len();

        // Determine CPU allocation strategy based on available cores
        // We have 2 pools (send/recv), each with `workers` threads
        let (workers, stride, offset, use_rt_sched) = if available_cpus >= requested_workers * 4 + 4
        {
            // Plenty of cores: use stride 4 for maximum separation + full RT priority
            // Send: 0, 4, 8, 12...  Recv: 2, 6, 10, 14...
            (requested_workers, 4, self.pool_type.base_offset() * 2, true)
        } else if available_cpus >= requested_workers * 2 + 4 {
            // Enough cores: use stride 2, interleaved + full RT priority
            // Send: 0, 2, 4, 6...  Recv: 1, 3, 5, 7...
            (requested_workers, 2, self.pool_type.base_offset(), true)
        } else {
            // Limited cores: cluster pools to avoid fragmentation + no RT scheduling
            // Send: 0, 2, 4...  Recv: starts after send pool
            let max_workers = (available_cpus.saturating_sub(4) / 2).max(1);
            let workers = requested_workers.min(max_workers);
            let offset = self.pool_type.base_offset() * workers * 2;
            (workers, 2, offset, false)
        };

        let pool_name = self.pool_type.name();

        if workers < requested_workers {
            tracing::warn!(
                pool_name,
                requested_workers,
                workers,
                available_cpus,
                "Limiting busy poll workers",
            );
        }

        tracing::info!(
            pool_name,
            workers,
            stride,
            cpu_start = offset,
            "Initializing busy poll workers",
        );

        let mut handles = Vec::with_capacity(workers);

        for idx in 0..workers {
            let (handle, runner) = busy_poll::Handle::new();
            let cpu_idx = offset + (idx * stride);

            // Map to actual allowed CPU ID
            let cpu_id = allowed_cpus.get(cpu_idx).copied().unwrap_or_else(|| {
                tracing::warn!(
                    "CPU index {} out of bounds for allowed set (len: {}), using fallback",
                    cpu_idx,
                    allowed_cpus.len()
                );
                allowed_cpus[cpu_idx % allowed_cpus.len()]
            });

            std::thread::Builder::new()
                .name(format!("dcquic:busy_poll:{}:{}", pool_name, idx))
                .spawn(move || {
                    Self::configure_thread(cpu_id, false);
                    runner.run()
                })
                .unwrap();

            handles.push(handle);
        }
        handles.into()
    }

    #[cfg(target_os = "linux")]
    fn get_allowed_cpus() -> Vec<usize> {
        unsafe {
            let mut allowed_set: libc::cpu_set_t = std::mem::zeroed();
            let result = libc::sched_getaffinity(
                0,
                std::mem::size_of::<libc::cpu_set_t>(),
                &mut allowed_set,
            );

            if result != 0 {
                tracing::warn!(
                    "Failed to get CPU affinity: {}, falling back to available_parallelism",
                    std::io::Error::last_os_error()
                );
                // Fallback: assume contiguous CPUs from 0
                return (0..std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(8))
                    .collect();
            }

            (0..libc::CPU_SETSIZE as usize)
                .filter(|&i| libc::CPU_ISSET(i, &allowed_set))
                .collect()
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn get_allowed_cpus() -> Vec<usize> {
        // Fallback for non-Linux: assume contiguous CPUs from 0
        (0..std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8))
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn configure_thread(cpu_id: usize, use_rt_sched: bool) {
        unsafe {
            // Set CPU affinity to pin thread to a specific core
            // let mut cpu_set: libc::cpu_set_t = std::mem::zeroed();
            // libc::CPU_SET(cpu_id, &mut cpu_set);
            // let result =
            //     libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpu_set);
            // if result != 0 {
            //     tracing::warn!(
            //         "Failed to set CPU affinity for busy poll thread to CPU {}: {}",
            //         cpu_id,
            //         std::io::Error::last_os_error()
            //     );
            // }

            if use_rt_sched {
                // Try to set high thread priority using SCHED_FIFO with conservative priority
                let max_prio = libc::sched_get_priority_max(libc::SCHED_FIFO);
                if max_prio < 0 {
                    tracing::warn!(
                        "Failed to get max RT priority for CPU {}: {}, falling back to nice priority",
                        cpu_id,
                        std::io::Error::last_os_error()
                    );
                } else {
                    // Use a conservative priority (50) to avoid starving critical kernel threads
                    // which typically run at RT priorities below 99 (watchdogs, RCU, migration, etc.)
                    let priority = max_prio.min(50);
                    let param = libc::sched_param {
                        sched_priority: priority,
                    };
                    let result = libc::sched_setscheduler(0, libc::SCHED_FIFO, &param);
                    if result == 0 {
                        // Successfully set RT priority, we're done
                        return;
                    }
                    tracing::warn!(
                        "Failed to set RT priority {} for busy poll thread on CPU {}: {}, falling back to nice priority",
                        priority,
                        cpu_id,
                        std::io::Error::last_os_error()
                    );
                }
            }

            // Fallback or explicit nice priority path
            let result = libc::setpriority(libc::PRIO_PROCESS, 0, -20);
            if result != 0 {
                tracing::warn!(
                    "Failed to set high nice priority for busy poll thread on CPU {}: {}",
                    cpu_id,
                    std::io::Error::last_os_error()
                );
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn configure_thread(_cpu_id: usize, _use_rt_sched: bool) {
        // No-op on non-Linux platforms
    }

    fn set_rtprio_limit() {
        #[cfg(target_os = "linux")]
        {
            use std::sync::Once;

            static RLIMIT: Once = Once::new();

            RLIMIT.call_once(|| {
                unsafe {
                    let mut rl: libc::rlimit = std::mem::zeroed();
                    rl.rlim_cur = 99;
                    rl.rlim_max = 99;

                    if libc::setrlimit(libc::RLIMIT_RTPRIO, &rl) != 0 {
                        tracing::warn!(
                            "Failed to set RLIMIT_RTPRIO: {}. RT scheduling will not work without CAP_SYS_NICE capability.",
                            std::io::Error::last_os_error()
                        );
                    } else {
                        tracing::debug!("Set RLIMIT_RTPRIO to 99");
                    }
                }
            });
        }
    }
}

impl core::ops::Deref for BusyPoll {
    type Target = busy_poll::Pool;

    fn deref(&self) -> &Self::Target {
        self.inner.get_or_init(|| self.init())
    }
}

pub static SEND_BUSY_POLL: BusyPoll = BusyPoll::new(PoolType::Send);
pub static RECV_BUSY_POLL: BusyPoll = BusyPoll::new(PoolType::Recv);

/// Returns a clone of the send busy poll pool
pub fn send_pool() -> busy_poll::Pool {
    SEND_BUSY_POLL.clone()
}

/// Returns a clone of the recv busy poll pool
pub fn recv_pool() -> busy_poll::Pool {
    RECV_BUSY_POLL.clone()
}

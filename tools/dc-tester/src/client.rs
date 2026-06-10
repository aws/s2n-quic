// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ClientConfig, PaxosPhaseConfig, WorkloadConfig};
use s2n_quic_core::{
    buffer::{reader::storage::Storage as _, Reader as _},
    stream::testing::Data,
    varint::VarInt,
};
use s2n_quic_dc::{
    counter,
    stream::{endpoint::Endpoint, MsgFlags},
};
use std::{
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::task::JoinSet;
use tracing::{info, warn};

pub async fn run(
    endpoint: Arc<Endpoint>,
    config: ClientConfig,
    server_addrs: Vec<SocketAddr>,
) -> io::Result<()> {
    if config.workloads.is_empty() {
        warn!("No workloads configured");
        return Ok(());
    }

    info!(
        server_addrs = ?server_addrs,
        workloads = %config.workloads.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", "),
        "Starting stream3 RPC test client"
    );
    for w in &config.workloads {
        if let Some(paxos) = &w.paxos {
            info!(
                name = %w.name,
                workers = w.workers,
                acceptors = paxos.acceptors,
                "  workload (paxos)"
            );
        } else {
            info!(
                name = %w.name,
                workers = w.workers,
                request_size = ?w.request_size,
                response_size = ?w.response_size,
                "  workload"
            );
        }
    }

    let data_addrs = endpoint.data_addrs.clone();

    // Create PSK client provider with data addrs
    let handshake = crate::psk::client(data_addrs, endpoint.path_secret_map.clone())?;

    // Create stream3 client
    let server_name = crate::psk::server_name();
    let mut client = s2n_quic_dc::stream::Client::new(endpoint.clone(), handshake, server_name);

    let stats = crate::stats::Subscriber::spawn(std::time::Duration::from_secs(1));

    // Warm up path secrets by sending a ping to each server
    for &addr in &server_addrs {
        info!(%addr, "Warming up connection");
        execute_single_message(&mut client, addr, 1, 1, false).await?;
    }
    info!("Warmup complete");

    let server_addrs: Arc<[SocketAddr]> = server_addrs.into();
    let mut handles = Vec::new();

    for workload in config.workloads {
        info!(
            workload = %workload.name,
            workers = workload.workers,
            "Starting workers"
        );

        let phase_timers = workload.paxos.as_ref().map(|_| {
            let prefix = format!("paxos.{}", workload.name);
            let make_phase = |name: &str| PhaseTimers {
                phase: endpoint
                    .counters
                    .register_timer(format!("{prefix}.{name}"))
                    .unsampled(),
                request: endpoint
                    .counters
                    .register_timer(format!("{prefix}.{name}.request"))
                    .unsampled(),
                laggard: endpoint
                    .counters
                    .register_timer(format!("{prefix}.{name}.laggard"))
                    .unsampled(),
                request_errors: endpoint
                    .counters
                    .register(format!("{prefix}.{name}.request_errors")),
            };
            PaxosTimers {
                prepare: make_phase("prepare"),
                accept: make_phase("accept"),
                learn: make_phase("learn"),
                round: endpoint
                    .counters
                    .register_timer(format!("{prefix}.round"))
                    .unsampled(),
                connect: endpoint
                    .counters
                    .register_timer(format!("{prefix}.connect"))
                    .unsampled(),
                send: endpoint
                    .counters
                    .register_timer(format!("{prefix}.send"))
                    .unsampled(),
                recv: endpoint
                    .counters
                    .register_timer(format!("{prefix}.recv"))
                    .unsampled(),
                rounds_completed: endpoint
                    .counters
                    .register(format!("{prefix}.rounds_completed")),
                rounds_failed: endpoint
                    .counters
                    .register(format!("{prefix}.rounds_failed")),
            }
        });

        for worker_id in 0..workload.workers {
            let mut client = client.clone();
            let workload = workload.clone();
            let stats = stats.clone();
            let phase_timers = phase_timers.clone();
            let server_addrs = server_addrs.clone();
            let handle = tokio::spawn(async move {
                run_worker(
                    &mut client,
                    &server_addrs,
                    workload,
                    worker_id,
                    stats,
                    phase_timers,
                )
                .await
            });
            handles.push(handle);
        }
    }

    // Wait for all workers (they run forever)
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

async fn run_worker(
    client: &mut s2n_quic_dc::stream::Client,
    server_addrs: &[SocketAddr],
    workload: WorkloadConfig,
    worker_id: usize,
    stats: crate::stats::Subscriber,
    phase_timers: Option<PaxosTimers>,
) {
    let delay = if workload.request_delay_ms > 0 {
        Some(Duration::from_millis(workload.request_delay_ms))
    } else {
        None
    };

    let mut rng = s2n_quic_dc::xorshift::Rng::new();
    let mut addr_index = worker_id % server_addrs.len();

    loop {
        let server_addr = server_addrs[addr_index];
        addr_index = (addr_index + 1) % server_addrs.len();

        stats.start_request();
        let (bytes_sent, bytes_received, is_error) = if workload.paxos.is_some() {
            let timers = phase_timers.as_ref().unwrap();
            match execute_paxos_round(
                client,
                server_addrs,
                &mut addr_index,
                &workload,
                &mut rng,
                timers,
            )
            .await
            {
                Ok((sent, received)) => {
                    timers.rounds_completed.add(1);
                    (sent, received, false)
                }
                Err(e) => {
                    timers.rounds_failed.add(1);
                    tracing::error!(
                        workload = %workload.name,
                        worker_id,
                        error = %e,
                        "Paxos round failed"
                    );
                    (0, 0, true)
                }
            }
        } else {
            match execute_request(client, server_addr, &workload, &mut rng).await {
                Ok((sent, received)) => (sent, received, false),
                Err(e) => {
                    tracing::error!(
                        workload = %workload.name,
                        worker_id,
                        error = %e,
                        "Request failed"
                    );
                    (0, 0, true)
                }
            }
        };
        stats.finish_request(bytes_sent, bytes_received, is_error);

        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
    }
}

async fn execute_request(
    client: &mut s2n_quic_dc::stream::Client,
    server_addr: SocketAddr,
    workload: &WorkloadConfig,
    rng: &mut s2n_quic_dc::xorshift::Rng,
) -> io::Result<(u64, u64)> {
    let request_size = workload.request_size.sample(rng);
    let response_size = workload.response_size.sample(rng);
    execute_single_message(
        client,
        server_addr,
        request_size,
        response_size,
        workload.use_msg,
    )
    .await
}

#[derive(Clone)]
struct PaxosTimers {
    prepare: PhaseTimers,
    accept: PhaseTimers,
    learn: PhaseTimers,
    round: counter::Timer,
    connect: counter::Timer,
    send: counter::Timer,
    recv: counter::Timer,
    rounds_completed: counter::Counter,
    rounds_failed: counter::Counter,
}

#[derive(Clone)]
struct PhaseTimers {
    phase: counter::Timer,
    request: counter::Timer,
    laggard: counter::Timer,
    request_errors: counter::Counter,
}

async fn execute_paxos_round(
    client: &mut s2n_quic_dc::stream::Client,
    server_addrs: &[SocketAddr],
    addr_index: &mut usize,
    workload: &WorkloadConfig,
    rng: &mut s2n_quic_dc::xorshift::Rng,
    timers: &PaxosTimers,
) -> io::Result<(u64, u64)> {
    let paxos = workload.paxos.as_ref().unwrap();
    let quorum = paxos.acceptors / 2 + 1;
    let mut total_sent = 0u64;
    let mut total_received = 0u64;

    let round_start = Instant::now();

    let (sent, received) = execute_phase(
        client,
        server_addrs,
        addr_index,
        &paxos.prepare,
        paxos.acceptors,
        quorum,
        workload.use_msg,
        rng,
        &timers.prepare,
        timers,
    )
    .await?;
    total_sent += sent;
    total_received += received;

    let (sent, received) = execute_phase(
        client,
        server_addrs,
        addr_index,
        &paxos.accept,
        paxos.acceptors,
        quorum,
        workload.use_msg,
        rng,
        &timers.accept,
        timers,
    )
    .await?;
    total_sent += sent;
    total_received += received;

    let (sent, received) = execute_phase(
        client,
        server_addrs,
        addr_index,
        &paxos.learn,
        paxos.acceptors,
        quorum,
        workload.use_msg,
        rng,
        &timers.learn,
        timers,
    )
    .await?;
    total_sent += sent;
    total_received += received;

    timers.round.record(round_start.elapsed());

    Ok((total_sent, total_received))
}

async fn execute_phase(
    client: &mut s2n_quic_dc::stream::Client,
    server_addrs: &[SocketAddr],
    addr_index: &mut usize,
    phase: &PaxosPhaseConfig,
    acceptors: usize,
    quorum: usize,
    use_msg: bool,
    rng: &mut s2n_quic_dc::xorshift::Rng,
    timers: &PhaseTimers,
    paxos_timers: &PaxosTimers,
) -> io::Result<(u64, u64)> {
    let sizes: Vec<(u64, u64)> = (0..acceptors)
        .map(|_| {
            (
                phase.request_size.sample(rng),
                phase.response_size.sample(rng),
            )
        })
        .collect();

    let phase_start = Instant::now();
    let laggard_elapsed_us = Arc::new(AtomicU64::new(0));

    let mut join_set = JoinSet::new();
    for (request_size, response_size) in sizes {
        let mut client = client.clone();
        let server_addr = server_addrs[*addr_index];
        *addr_index = (*addr_index + 1) % server_addrs.len();
        let request_timer = timers.request.clone();
        let laggard_elapsed_us = laggard_elapsed_us.clone();
        let request_errors = timers.request_errors.clone();
        let connect_timer = paxos_timers.connect.clone();
        let send_timer = paxos_timers.send.clone();
        let recv_timer = paxos_timers.recv.clone();
        join_set.spawn(async move {
            let req_start = Instant::now();
            let result = execute_single_message_instrumented(
                &mut client,
                server_addr,
                request_size,
                response_size,
                use_msg,
                &connect_timer,
                &send_timer,
                &recv_timer,
            )
            .await;
            let elapsed = req_start.elapsed();
            request_timer.record(elapsed);
            laggard_elapsed_us.fetch_max(elapsed.as_micros() as u64, Ordering::Relaxed);
            if result.is_err() {
                request_errors.add(1);
            }
            result
        });
    }

    let mut total_sent = 0u64;
    let mut total_received = 0u64;
    let mut successes = 0;
    let mut last_error = None;

    // JoinSet returns results in completion order — quorum is reached as soon as
    // the fastest majority finishes, no head-of-line blocking on a slow acceptor.
    while let Some(result) = join_set.join_next().await {
        match result.map_err(io::Error::other)? {
            Ok((sent, received)) => {
                total_sent += sent;
                total_received += received;
                successes += 1;
                if successes >= quorum {
                    break;
                }
            }
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    if successes < quorum {
        return Err(last_error.unwrap_or_else(|| {
            io::Error::other(format!(
                "quorum not reached: {successes}/{quorum} succeeded"
            ))
        }));
    }

    // Detach remaining tasks so they finish and record their laggard time
    // rather than being aborted when the JoinSet drops.
    join_set.detach_all();

    timers.phase.record(phase_start.elapsed());
    timers.laggard.record(Duration::from_micros(
        laggard_elapsed_us.load(Ordering::Relaxed),
    ));

    Ok((total_sent, total_received))
}

async fn execute_single_message(
    client: &mut s2n_quic_dc::stream::Client,
    server_addr: SocketAddr,
    request_size: u64,
    response_size: u64,
    use_msg: bool,
) -> io::Result<(u64, u64)> {
    let stream = client
        .connect(
            server_addr,
            VarInt::ZERO,
            s2n_quic_dc::credit::Priority::default(),
        )
        .await?;
    send_recv(stream, request_size, response_size, use_msg).await
}

async fn execute_single_message_instrumented(
    client: &mut s2n_quic_dc::stream::Client,
    server_addr: SocketAddr,
    request_size: u64,
    response_size: u64,
    use_msg: bool,
    connect_timer: &counter::Timer,
    send_timer: &counter::Timer,
    recv_timer: &counter::Timer,
) -> io::Result<(u64, u64)> {
    let connect_start = Instant::now();
    let stream = client
        .connect(
            server_addr,
            VarInt::ZERO,
            s2n_quic_dc::credit::Priority::default(),
        )
        .await?;
    connect_timer.record(connect_start.elapsed());

    let (mut reader, mut writer) = stream.into_split();

    let wire_response_size = if use_msg {
        response_size | USE_MSG_FLAG
    } else {
        response_size
    };

    let send_start = Instant::now();
    let send = async move {
        let header = wire_response_size.to_be_bytes();
        let mut payload = (&header[..]).chain(Data::new(request_size));
        if use_msg {
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await?;
        } else {
            loop {
                if payload.buffer_is_empty() {
                    break;
                }
                tokio::time::timeout(Duration::from_secs(10), writer.write_from_fin(&mut payload))
                    .await
                    .expect("writer did not produce a chunk within 10 seconds")?;
            }
        }
        io::Result::Ok(8 + request_size)
    };

    let recv = async move {
        let mut response = Data::new(response_size);
        loop {
            let n = match tokio::time::timeout(
                Duration::from_secs(10),
                reader.read_into(&mut response),
            )
            .await
            {
                Ok(result) => result?,
                Err(_elapsed) => {
                    match tokio::time::timeout(
                        Duration::from_millis(1),
                        reader.read_into(&mut response),
                    )
                    .await
                    {
                        Ok(Ok(n)) if n > 0 => {
                            panic!(
                                "BUG: missed waker! read {n} bytes on immediate retry \
                                 after 10s timeout. offset={}/{}",
                                response.current_offset(),
                                response_size,
                            );
                        }
                        _ => {
                            panic!(
                                "reader did not produce a chunk within 10 seconds \
                                 and no data was available on retry. offset={}/{}",
                                response.current_offset(),
                                response_size,
                            );
                        }
                    }
                }
            };
            if n == 0 {
                break;
            }
        }

        if !response.is_finished() {
            return Err(io::Error::other(format!(
                "response was not fully received: expected {} bytes, got {} bytes",
                response_size,
                response.current_offset()
            )));
        }

        io::Result::Ok(response_size)
    };

    let (bytes_sent, bytes_received) =
        if request_size >= SPAWN_THRESHOLD && response_size >= SPAWN_THRESHOLD {
            let send = tokio::spawn(send);
            let recv = tokio::spawn(recv);
            tokio::try_join!(async { send.await.expect("send task panicked") }, async {
                recv.await.expect("recv task panicked")
            },)?
        } else {
            tokio::try_join!(send, recv)?
        };
    send_timer.record(send_start.elapsed());
    recv_timer.record(send_start.elapsed());
    Ok((bytes_sent, bytes_received))
}

pub(crate) const SPAWN_THRESHOLD: u64 = 1024 * 1024;

/// High bit of the response_size header signals the server should use write_msg.
pub(crate) const USE_MSG_FLAG: u64 = 1 << 63;

async fn send_recv(
    stream: s2n_quic_dc::stream::Stream,
    request_size: u64,
    response_size: u64,
    use_msg: bool,
) -> io::Result<(u64, u64)> {
    let (mut reader, mut writer) = stream.into_split();

    let wire_response_size = if use_msg {
        response_size | USE_MSG_FLAG
    } else {
        response_size
    };

    let send = async move {
        let header = wire_response_size.to_be_bytes();
        let mut payload = (&header[..]).chain(Data::new(request_size));
        if use_msg {
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await?;
        } else {
            loop {
                if payload.buffer_is_empty() {
                    break;
                }
                tokio::time::timeout(Duration::from_secs(10), writer.write_from_fin(&mut payload))
                    .await
                    .expect("writer did not produce a chunk within 10 seconds")?;
            }
        }
        io::Result::Ok(8 + request_size)
    };

    let recv = async move {
        let mut response = Data::new(response_size);
        loop {
            let n = match tokio::time::timeout(
                Duration::from_secs(10),
                reader.read_into(&mut response),
            )
            .await
            {
                Ok(result) => result?,
                Err(_elapsed) => {
                    match tokio::time::timeout(
                        Duration::from_millis(1),
                        reader.read_into(&mut response),
                    )
                    .await
                    {
                        Ok(Ok(n)) if n > 0 => {
                            panic!(
                                "BUG: missed waker! read {n} bytes on immediate retry \
                                 after 10s timeout. offset={}/{}",
                                response.current_offset(),
                                response_size,
                            );
                        }
                        _ => {
                            panic!(
                                "reader did not produce a chunk within 10 seconds \
                                 and no data was available on retry. offset={}/{}",
                                response.current_offset(),
                                response_size,
                            );
                        }
                    }
                }
            };
            if n == 0 {
                break;
            }
        }

        if !response.is_finished() {
            return Err(io::Error::other(format!(
                "response was not fully received: expected {} bytes, got {} bytes",
                response_size,
                response.current_offset()
            )));
        }

        io::Result::Ok(response_size)
    };

    if request_size >= SPAWN_THRESHOLD && response_size >= SPAWN_THRESHOLD {
        let send = tokio::spawn(send);
        let recv = tokio::spawn(recv);
        let (bytes_sent, bytes_received) =
            tokio::try_join!(async { send.await.expect("send task panicked") }, async {
                recv.await.expect("recv task panicked")
            },)?;
        Ok((bytes_sent, bytes_received))
    } else {
        let (bytes_sent, bytes_received) = tokio::try_join!(send, recv)?;
        Ok((bytes_sent, bytes_received))
    }
}

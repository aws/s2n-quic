// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{self, EndpointPublisher, IntoEvent, Subscriber},
    path::secret,
    stream::{
        environment::{tokio::Environment, Environment as _},
        server::{accept, tokio::tcp::worker::PollBehavior},
    },
};
use core::{future::poll_fn, task::Poll};
use s2n_quic_core::{inet::SocketAddress, time::Clock};
use std::{net::TcpListener, time::Duration};
use tokio::io::unix::AsyncFd;
use tracing::debug;

mod fresh;
mod lazy;
mod manager;
pub mod tls;
pub mod worker;

pub(crate) use lazy::LazyBoundStream;

pub struct Acceptor<Sub, B>
where
    Sub: Subscriber + Clone,
    B: PollBehavior<Sub> + Clone,
{
    socket: AsyncFd<TcpListener>,
    env: Environment<Sub>,
    secrets: secret::Map,
    backlog: usize,
    accept_flavor: accept::Flavor,
    linger: Option<Duration>,
    poll_behavior: B,
}

impl<Sub, B> Acceptor<Sub, B>
where
    Sub: event::Subscriber + Clone,
    B: PollBehavior<Sub> + Clone,
{
    #[inline]
    pub fn new(
        id: usize,
        socket: AsyncFd<TcpListener>,
        env: &Environment<Sub>,
        secrets: &secret::Map,
        backlog: usize,
        accept_flavor: accept::Flavor,
        linger: Option<Duration>,
        poll_behavior: B,
    ) -> std::io::Result<Self> {
        let acceptor = Self {
            socket,
            env: env.clone(),
            secrets: secrets.clone(),
            backlog,
            accept_flavor,
            linger,
            poll_behavior,
        };

        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;

            let res = unsafe {
                libc::setsockopt(
                    acceptor.socket.get_ref().as_raw_fd(),
                    libc::SOL_TCP,
                    libc::TCP_DEFER_ACCEPT,
                    // This is how many seconds elapse before the kernel will accept a stream
                    // without any data and return it to userspace. Any number of seconds is
                    // arguably too many in our domain (we'd expect data in milliseconds) but in
                    // practice this value shouldn't matter much.
                    &1u32 as *const _ as *const _,
                    std::mem::size_of::<u32>() as libc::socklen_t,
                )
            };
            if res != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }

        if let Ok(addr) = acceptor.socket.get_ref().local_addr() {
            let local_address: SocketAddress = addr.into();
            acceptor.env.endpoint_publisher().on_acceptor_tcp_started(
                event::builder::AcceptorTcpStarted {
                    id,
                    local_address: &local_address,
                    backlog,
                },
            );
        }

        Ok(acceptor)
    }

    pub async fn run(mut self) {
        let drop_guard = DropLog;
        let mut fresh = fresh::Queue::new(self.backlog);
        let mut workers = {
            let workers = (0..self.backlog).map(|_| {
                worker::Worker::<Sub, B>::new(
                    self.env.clock().get_time(),
                    self.poll_behavior.clone(),
                )
            });
            manager::Manager::new(workers)
        };
        let mut context = worker::Context::new(&self);

        poll_fn(move |cx| {
            workers.poll_start(cx);

            let now = self.env.clock().get_time();
            let publisher = self.env.endpoint_publisher_with_time(now);

            fresh.fill(cx, &mut self.socket, &publisher);

            for (socket, remote_address) in fresh.drain() {
                let meta = event::api::ConnectionMeta {
                    id: 0, // TODO use an actual connection ID
                    timestamp: now.into_event(),
                };
                let info = event::api::ConnectionInfo {};

                let subscriber_ctx = self
                    .env
                    .subscriber()
                    .create_connection_context(&meta, &info);

                workers.insert(
                    remote_address,
                    LazyBoundStream::Std(socket),
                    self.linger,
                    &mut context,
                    subscriber_ctx,
                    &publisher,
                    &now,
                );
            }

            let res = workers.poll(&mut context, &publisher, &now);

            publisher.on_acceptor_tcp_loop_iteration_completed(
                event::builder::AcceptorTcpLoopIterationCompleted {
                    pending_streams: workers.active_slots(),
                    slots_idle: workers.free_slots(),
                    slot_utilization: (workers.active_slots() as f32 / workers.capacity() as f32)
                        * 100.0,
                    processing_duration: self.env.clock().get_time().saturating_duration_since(now),
                    max_sojourn_time: workers.max_sojourn_time(),
                },
            );

            if res.is_continue() {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;

        drop(drop_guard);
    }
}

struct DropLog;

impl Drop for DropLog {
    #[inline]
    fn drop(&mut self) {
        debug!("acceptor task has been dropped");
    }
}

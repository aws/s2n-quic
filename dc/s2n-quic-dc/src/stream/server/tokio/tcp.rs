// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{self, EndpointPublisher, IntoEvent, Subscriber},
    path::secret,
    stream::{
        environment::{tokio::Environment, Environment as _},
        server::accept,
    },
};
use core::{future::poll_fn, task::Poll};
use s2n_quic_core::{inet::SocketAddress, time::Clock};
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::debug;

mod fresh;
mod manager;
mod worker;

pub struct Acceptor<Sub>
where
    Sub: Subscriber + Clone,
{
    sender: accept::Sender<Sub>,
    socket: TcpListener,
    env: Environment<Sub>,
    secrets: secret::Map,
    backlog: usize,
    accept_flavor: accept::Flavor,
    linger: Option<Duration>,
}

impl<Sub> Acceptor<Sub>
where
    Sub: event::Subscriber + Clone,
{
    #[inline]
    pub fn new(
        id: usize,
        socket: TcpListener,
        sender: &accept::Sender<Sub>,
        env: &Environment<Sub>,
        secrets: &secret::Map,
        backlog: usize,
        accept_flavor: accept::Flavor,
        linger: Option<Duration>,
    ) -> Self {
        let acceptor = Self {
            sender: sender.clone(),
            socket,
            env: env.clone(),
            secrets: secrets.clone(),
            backlog,
            accept_flavor,
            linger,
        };

        if let Ok(addr) = acceptor.socket.local_addr() {
            let local_address: SocketAddress = addr.into();
            acceptor.env.endpoint_publisher().on_acceptor_tcp_started(
                event::builder::AcceptorTcpStarted {
                    id,
                    local_address: &local_address,
                    backlog,
                },
            );
        }

        acceptor
    }

    pub async fn run(mut self) {
        let drop_guard = DropLog;
        let mut fresh = fresh::Queue::new(self.backlog);
        let mut workers = {
            let workers =
                (0..self.backlog).map(|_| worker::Worker::new(self.env.clock().get_time()));
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
                    socket,
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

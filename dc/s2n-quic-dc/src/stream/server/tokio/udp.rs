// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::accept;
use crate::{
    msg,
    path::secret,
    stream::{
        endpoint,
        environment::{
            tokio::{self as env, Environment},
            Environment as _,
        },
        server,
        socket::{Ext as _, Socket},
    },
};
use core::ops::ControlFlow;
use s2n_quic_core::time::Clock as _;
use std::io;
use tracing::debug;

pub struct Acceptor<S: Socket> {
    sender: accept::Sender,
    socket: S,
    recv_buffer: msg::recv::Message,
    handshake: server::handshake::Map,
    env: Environment,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
}

impl<S: Socket> Acceptor<S> {
    #[inline]
    pub fn new(
        socket: S,
        sender: &accept::Sender,
        env: &Environment,
        secrets: &secret::Map,
        accept_flavor: accept::Flavor,
    ) -> Self {
        Self {
            sender: sender.clone(),
            socket,
            recv_buffer: msg::recv::Message::new(9000.try_into().unwrap()),
            handshake: Default::default(),
            env: env.clone(),
            secrets: secrets.clone(),
            accept_flavor,
        }
    }

    pub async fn run(mut self) {
        loop {
            match self.accept_one().await {
                Ok(ControlFlow::Continue(())) => continue,
                Ok(ControlFlow::Break(())) => break,
                Err(err) => {
                    tracing::error!(acceptor_error = %err);
                }
            }
        }
    }

    async fn accept_one(&mut self) -> io::Result<ControlFlow<()>> {
        let packet = self.recv_packet().await?;

        let now = self.env.clock().get_time();

        let server::handshake::Outcome::Created {
            receiver: handshake,
        } = self.handshake.handle(&packet, &mut self.recv_buffer)
        else {
            return Ok(ControlFlow::Continue(()));
        };

        let remote_addr = self.recv_buffer.remote_address();
        let stream = match endpoint::accept_stream(
            &self.env,
            env::UdpUnbound(remote_addr),
            &packet,
            Some(handshake),
            Some(&mut self.recv_buffer),
            &self.secrets,
            None,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::trace!("send_start");

                let addr = msg::addr::Addr::new(remote_addr);
                let ecn = Default::default();
                let buffer = &[io::IoSlice::new(&error.secret_control)];

                // ignore any errors since this is just for responding to invalid connect attempts
                let _ = self.socket.try_send(&addr, ecn, buffer);

                tracing::trace!("send_finish");
                return Err(error.error);
            }
        };

        let item = (stream, now);
        let res = match self.accept_flavor {
            accept::Flavor::Fifo => self.sender.send_back(item),
            accept::Flavor::Lifo => self.sender.send_front(item),
        };

        match res {
            Ok(prev) => {
                if let Some((stream, queue_time)) = prev {
                    debug!(
                        event = "accept::prune",
                        credentials = ?stream.shared.credentials(),
                        queue_duration = ?now.saturating_duration_since(queue_time),
                    );
                }

                Ok(ControlFlow::Continue(()))
            }
            Err(_) => {
                debug!("application accept queue dropped; shutting down");
                Ok(ControlFlow::Break(()))
            }
        }
    }

    async fn recv_packet(&mut self) -> io::Result<server::InitialPacket> {
        loop {
            // discard any pending packets
            self.recv_buffer.clear();
            tracing::trace!("recv_start");
            self.socket.recv_buffer(&mut self.recv_buffer).await?;
            tracing::trace!("recv_finish");

            match server::InitialPacket::peek(&mut self.recv_buffer, 16) {
                Ok(initial_packet) => {
                    tracing::debug!(?initial_packet);
                    return Ok(initial_packet);
                }
                Err(initial_packet_error) => {
                    tracing::debug!(?initial_packet_error);
                    continue;
                }
            }
        }
    }
}

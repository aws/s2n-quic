// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Forwards UDP streams to an application without tracking open streams.
//!
//! The shared path-secret map allows one authenticated credential handoff per stream.
//! Later packets are forwarded without secrets. Failed handoff enqueues have bounded retries.
//!
//! Only the application sends stream/control replies, so clients follow its port and bypass
//! the manager. Unknown-secret recovery responses can still terminate stale streams.

use crate::{
    credentials::Credentials,
    crypto::{self, open::Application as _},
    event::{self, EndpointPublisher as _, IntoEvent as _},
    packet::{self, stream},
    path::secret::{self, map::Bidirectional, schedule::Ciphersuite},
    socket::recv::{descriptor, router::Router},
    stream::environment::Environment,
    uds::sender::Sender,
};
use nix::{sys::time::TimeValLike as _, time::ClockId};
use s2n_codec::{DecoderBufferMut, EncoderBuffer, EncoderLenEstimator};
use s2n_quic_core::{
    dc::ApplicationParams,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::{Clock as _, Timestamp},
    varint::VarInt,
};
use std::{io, net::UdpSocket, path::Path, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// At most 4 MiB of encoded, unsent handoffs per socket (64 * u16::MAX).
const MAX_PENDING_HANDOFFS: usize = 64;
const HANDOFF_SEND_TIMEOUT: Duration = Duration::from_secs(30);
const RECONNECT_DELAY: Duration = Duration::from_millis(10);

struct Handoff {
    ciphersuite: Ciphersuite,
    export_secret: secret::schedule::ExportSecret,
    application_params: ApplicationParams,
    application_data: Option<Vec<u8>>,
    permit: OwnedSemaphorePermit,
}

pub struct Acceptor<Env: Environment> {
    env: Env,
    secrets: secret::Map,
    socket: Arc<UdpSocket>,
    local_address: SocketAddress,
    socket_path: PathBuf,
    sender: Option<Sender>,
    delivery_capacity: Arc<Semaphore>,
    current_handoff: Option<Handoff>,
}

impl<Env: Environment> Acceptor<Env> {
    pub fn new(
        env: Env,
        secrets: secret::Map,
        socket: Arc<UdpSocket>,
        socket_path: PathBuf,
    ) -> io::Result<Self> {
        let local_address = socket.local_addr()?.into();
        let sender = Some(Sender::new(&socket_path)?);
        Ok(Self {
            env,
            secrets,
            socket,
            local_address,
            socket_path,
            sender,
            delivery_capacity: Arc::new(Semaphore::new(MAX_PENDING_HANDOFFS)),
            current_handoff: None,
        })
    }

    fn admit(
        &mut self,
        remote_address: SocketAddress,
        packet: &stream::decoder::Packet,
    ) -> Option<Handoff> {
        let mut secret_control = vec![];
        let Some((export_secret, ciphersuite, keys, application_params, application_data)) =
            self.secrets.secret_for_handoff(
                packet.credentials(),
                packet.source_queue_id(),
                &mut secret_control,
            )
        else {
            if !secret_control.is_empty() {
                let _ = self
                    .socket
                    .send_to(&secret_control, std::net::SocketAddr::from(remote_address));
            }
            return None;
        };

        // Reserve delivery capacity before authentication consumes the replay claim.
        let permit = self.delivery_capacity.clone().try_acquire_owned().ok()?;

        if let Err(error) = authenticate(keys, packet) {
            tracing::debug!(?error, "forwarding packet without a new handoff");
            return None;
        }

        let application_data = match application_data
            .as_ref()
            .map(|data| self.secrets.serialize_application_data(data))
        {
            None | Some(Ok(None)) => None,
            Some(Ok(Some(bytes))) => Some(bytes),
            Some(Err(err)) => {
                tracing::warn!(
                    ?err,
                    "failed to serialize application data; forwarding without it"
                );
                None
            }
        };

        Some(Handoff {
            ciphersuite,
            export_secret,
            application_params,
            application_data,
            permit,
        })
    }

    fn handoff(
        &mut self,
        handoff: Handoff,
        credentials: Credentials,
        remote_address: SocketAddress,
        segment: &descriptor::Filled,
    ) {
        let packet = encode_time().and_then(|encode_time| {
            encode_handoff(
                &handoff,
                encode_time,
                &remote_address,
                &self.local_address,
                segment.payload(),
            )
        });
        let packet = match packet {
            Ok(packet) => packet,
            Err(error) => {
                self.on_io_error(&error);
                self.forward(credentials, remote_address, segment);
                return;
            }
        };

        match self.enqueue(&packet) {
            Ok(()) => publish_handoff(
                self.env.subscriber(),
                self.env.clock().get_time(),
                &credentials,
                &remote_address,
                packet.len(),
            ),
            Err(error) => {
                self.on_io_error(&error);
                if !retryable(&error) {
                    return;
                }

                let timeout = handoff
                    .application_params
                    .max_idle_timeout()
                    .unwrap_or(HANDOFF_SEND_TIMEOUT)
                    .min(HANDOFF_SEND_TIMEOUT);
                let permit = handoff.permit;
                let sender = self.sender.clone();
                let socket_path = self.socket_path.clone();
                let subscriber = self.env.subscriber().clone();
                let clock = self.env.clock();
                let deadline = tokio::time::Instant::now() + timeout;

                // Use the acceptor runtime that owns the socket's reactor.
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = tokio::time::timeout_at(
                        deadline,
                        retry_handoff(sender, &socket_path, &packet, deadline),
                    )
                    .await
                    .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()));
                    let now = clock.get_time();
                    match result {
                        Ok(()) => publish_handoff(
                            &subscriber,
                            now,
                            &credentials,
                            &remote_address,
                            packet.len(),
                        ),
                        Err(error) => {
                            let publisher = event::EndpointPublisherSubscriber::new(
                                event::builder::EndpointMeta {
                                    timestamp: now.into_event(),
                                },
                                None,
                                &subscriber,
                            );
                            publisher.on_acceptor_udp_io_error(
                                event::builder::AcceptorUdpIoError { error: &error },
                            );
                        }
                    }
                });
            }
        }
    }

    fn forward(
        &mut self,
        credentials: Credentials,
        remote_address: SocketAddress,
        segment: &descriptor::Filled,
    ) {
        let Ok(encode_time) = encode_time() else {
            return;
        };
        let payload = segment.payload();
        let size = packet::uds::encoder::encode_forward(
            &mut EncoderLenEstimator::new(usize::MAX),
            encode_time,
            &remote_address,
            payload,
        );
        if size > u16::MAX as usize {
            self.on_io_error(&io::Error::new(
                io::ErrorKind::InvalidData,
                "forwarded packet exceeds the UDS receive buffer",
            ));
            return;
        }
        let mut buffer = vec![0u8; size];
        packet::uds::encoder::encode_forward(
            &mut EncoderBuffer::new(&mut buffer),
            encode_time,
            &remote_address,
            payload,
        );

        match self.enqueue(&buffer) {
            Ok(()) => self
                .env
                .endpoint_publisher()
                .on_acceptor_udp_packet_forwarded(event::builder::AcceptorUdpPacketForwarded {
                    remote_address: &remote_address,
                    credential_id: &*credentials.id,
                    stream_id: credentials.key_id.as_u64(),
                }),
            Err(error) => self.on_io_error(&error),
        }
    }

    fn enqueue(&mut self, packet: &[u8]) -> io::Result<()> {
        let mut result = match &self.sender {
            Some(sender) => sender.try_send(packet),
            None => Err(io::ErrorKind::NotConnected.into()),
        };

        if matches!(
            result.as_ref().map_err(io::Error::kind),
            Err(io::ErrorKind::ConnectionRefused
                | io::ErrorKind::NotFound
                | io::ErrorKind::NotConnected
                | io::ErrorKind::BrokenPipe)
        ) {
            self.sender = Sender::new(&self.socket_path).ok();
            if let Some(sender) = &self.sender {
                result = sender.try_send(packet);
            }
        }
        result
    }

    fn on_io_error(&self, error: &io::Error) {
        self.env
            .endpoint_publisher()
            .on_acceptor_udp_io_error(event::builder::AcceptorUdpIoError { error });
    }
}

impl<Env: Environment> Router for Acceptor<Env> {
    #[inline]
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn handle_stream_packet(
        &mut self,
        remote_address: SocketAddress,
        _ecn: ExplicitCongestionNotification,
        packet: stream::decoder::Packet,
    ) {
        self.current_handoff = if packet.stream_id().queue_id() == VarInt::ZERO {
            self.admit(remote_address, &packet)
        } else {
            None
        };
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        _tag: stream::Tag,
        _id: stream::Id,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        let remote_address = segment.remote_address().get();
        match self.current_handoff.take() {
            Some(handoff) => self.handoff(handoff, credentials, remote_address, &segment),
            None => self.forward(credentials, remote_address, &segment),
        }
    }

    #[inline]
    fn handle_control_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: ExplicitCongestionNotification,
        _packet: packet::control::decoder::Packet,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        _tag: packet::control::Tag,
        id: Option<stream::Id>,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        if id.is_some() {
            self.forward(credentials, segment.remote_address().get(), &segment);
        }
    }
}

async fn retry_handoff(
    mut sender: Option<Sender>,
    socket_path: &Path,
    packet: &[u8],
    deadline: tokio::time::Instant,
) -> io::Result<()> {
    loop {
        // Reject expired retries even if the socket is already writable.
        if tokio::time::Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if sender.is_none() {
            match Sender::new(socket_path) {
                Ok(connected) => sender = Some(connected),
                Err(error) if retryable(&error) => {
                    tokio::time::sleep(RECONNECT_DELAY).await;
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        if let Some(connected) = &sender {
            match connected.send(packet).await {
                Ok(()) => return Ok(()),
                Err(error) if retryable(&error) => {
                    sender = None;
                    tokio::time::sleep(RECONNECT_DELAY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock
            | io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::NotFound
            | io::ErrorKind::NotConnected
            | io::ErrorKind::BrokenPipe
    )
}

fn publish_handoff<Sub: event::Subscriber>(
    subscriber: &Sub,
    now: Timestamp,
    credentials: &Credentials,
    remote_address: &SocketAddress,
    payload_len: usize,
) {
    let publisher = event::EndpointPublisherSubscriber::new(
        event::builder::EndpointMeta {
            timestamp: now.into_event(),
        },
        None,
        subscriber,
    );
    publisher.on_acceptor_udp_stream_handed_off(event::builder::AcceptorUdpStreamHandedOff {
        remote_address,
        credential_id: &*credentials.id,
        stream_id: credentials.key_id.as_u64(),
        payload_len,
    });
}

fn encode_handoff(
    handoff: &Handoff,
    encode_time: u64,
    remote_address: &SocketAddress,
    local_address: &SocketAddress,
    payload: &[u8],
) -> io::Result<Vec<u8>> {
    let size = |application_data: Option<&[u8]>| {
        packet::uds::encoder::encode_handoff(
            &mut EncoderLenEstimator::new(usize::MAX),
            packet::uds::Protocol::Udp,
            &handoff.ciphersuite,
            &handoff.export_secret,
            &handoff.application_params,
            encode_time,
            remote_address,
            Some(local_address),
            application_data,
            payload,
        )
    };

    let mut application_data = handoff.application_data.as_deref();
    let mut len = size(application_data);
    if len > u16::MAX as usize && application_data.is_some() {
        tracing::warn!(
            len,
            "UDS handoff exceeds receive buffer with application data; forwarding without it"
        );
        application_data = None;
        len = size(application_data);
    }
    if len > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "handoff exceeds the UDS receive buffer",
        ));
    }

    let mut buffer = vec![0u8; len];
    packet::uds::encoder::encode_handoff(
        &mut EncoderBuffer::new(&mut buffer),
        packet::uds::Protocol::Udp,
        &handoff.ciphersuite,
        &handoff.export_secret,
        &handoff.application_params,
        encode_time,
        remote_address,
        Some(local_address),
        application_data,
        payload,
    );
    Ok(buffer)
}

fn authenticate(keys: Bidirectional, packet: &stream::decoder::Packet) -> crypto::open::Result {
    // Control-only probes cannot claim an application key.
    if packet.tag().packet_space().is_recovery() && !packet.is_retransmission() {
        return Err(crypto::open::Error::MacOnly);
    }
    let mut payload_out = vec![0u8; packet.payload().len()];
    let payload_out = crypto::UninitSlice::new(&mut payload_out);
    if !packet.is_retransmission() {
        return keys.application.opener.decrypt(
            packet.tag().key_phase(),
            *packet.packet_number(),
            packet.header(),
            packet.payload(),
            packet.auth_tag(),
            payload_out,
        );
    }

    // Normalize retransmissions for AEAD without changing the packet sent to the application.
    let control = keys
        .control
        .as_ref()
        .ok_or(crypto::open::Error::UnsupportedOperation)?;
    let mut scratch = Vec::with_capacity(packet.total_len());
    scratch.extend_from_slice(packet.header());
    scratch.extend_from_slice(packet.payload());
    scratch.extend_from_slice(packet.auth_tag());
    let (mut packet, _) = stream::decoder::Packet::decode(
        DecoderBufferMut::new(&mut scratch),
        (),
        keys.application.opener.tag_len(),
    )
    .map_err(|_| crypto::open::Error::InvalidTag)?;
    packet.decrypt(&keys.application.opener, &control.opener, payload_out)
}

fn encode_time() -> io::Result<u64> {
    #[cfg(target_os = "linux")]
    let clock = ClockId::CLOCK_MONOTONIC_RAW;

    #[cfg(not(target_os = "linux"))]
    let clock = ClockId::CLOCK_MONOTONIC;

    Ok(clock.now().map_err(io::Error::from)?.num_microseconds() as u64)
}

#[cfg(test)]
mod tests;

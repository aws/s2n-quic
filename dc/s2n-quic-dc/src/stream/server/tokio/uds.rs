// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    either::{self, Either},
    event::{self, EndpointPublisher, Subscriber},
    msg::recv::Message,
    packet::uds::{decoder, Protocol},
    path::secret::{
        map::{
            ApplicationData, ApplicationDataError, ApplicationPair, Bidirectional, ControlPair,
            Dedup,
        },
        schedule::{ExportSecret, Initiator, Secret},
        stateless_reset, Map,
    },
    socket::recv::{descriptor, pool::Pool as Packets},
    stream::{
        application::Builder,
        endpoint,
        environment::{
            tokio::{self as env, Environment},
            Environment as _,
        },
        recv::{self, buffer::Channel},
        server::{self, tokio::tcp::LazyBoundStream},
    },
    uds::{self},
};
use nix::{sys::time::TimeValLike as _, time::ClockId};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    endpoint::Type,
    event::IntoEvent as _,
    inet::SocketAddress,
    time::{self, Clock as _},
};
use std::{
    io::{self, ErrorKind},
    os::fd::OwnedFd,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpStream;

/// Reconstructs the serialized blob carried in a UDS handoff packet back into the type-erased
/// [`ApplicationData`] associated with the stream's path secret.
///
/// This is the value produced by
/// [`Map::register_make_application_data`](crate::path::secret::map::Map::register_make_application_data)
/// on the forwarding side, not payload sent by the application over the stream. The dc crate
/// never inspects the bytes; the callback is the application's inverse of the serializer
/// registered with
/// [`Map::register_application_data_serializer`](crate::path::secret::map::Map::register_application_data_serializer).
/// Returning `Ok(None)` or `Err` results in the stream being accepted with
/// `application_data: None` (fail-open).
pub type ApplicationDataDeserializer =
    Arc<dyn Fn(&[u8]) -> Result<Option<ApplicationData>, ApplicationDataError> + Send + Sync>;

#[derive(Clone)]
pub struct Receiver<Sub>
where
    Sub: Subscriber + Clone,
{
    receiver: uds::receiver::Receiver,
    env: Environment<Sub>,
    map: Map, // placeholder map
    application_data_deserializer: Option<ApplicationDataDeserializer>,
    // Receive descriptors for handed-off UDP packets
    packets: Arc<Mutex<Packets>>,
}

impl<Sub> Receiver<Sub>
where
    Sub: Subscriber + Clone,
{
    pub fn new(socket_path: &Path, env: &Environment<Sub>) -> std::io::Result<Self> {
        let map = placeholder_map(env.subscriber().clone());
        Self::new_with_map(socket_path, env, map)
    }

    /// Creates a receiver using `map`, which must be the map `env`'s receive pool was built with.
    pub fn new_with_map(
        socket_path: &Path,
        env: &Environment<Sub>,
        map: Map,
    ) -> std::io::Result<Self> {
        let receiver = uds::receiver::Receiver::new(socket_path)?;
        Ok(Self {
            receiver,
            env: env.clone(),
            map,
            application_data_deserializer: None,
            packets: Arc::new(Mutex::new(Packets::new(u16::MAX, 16))),
        })
    }

    /// Registers the callback used to reconstruct the path secret's [`ApplicationData`] from the
    /// opaque blob carried in a UDS handoff packet. When unset, accepted streams are accepted with
    /// `application_data: None`.
    pub fn with_application_data_deserializer(
        mut self,
        deserializer: ApplicationDataDeserializer,
    ) -> Self {
        self.application_data_deserializer = Some(deserializer);
        self
    }

    pub async fn receive_stream(&self) -> std::io::Result<Builder<Sub>> {
        loop {
            let (mut packet_data, fd) = self.receiver.receive_msg().await?;

            if decoder::Forward::is_forward(&packet_data) {
                self.receive_forward(&mut packet_data);
                continue;
            }

            let decoded_packet = Self::decode_packet(&mut packet_data)?;
            let payload_len = packet_data.len();

            match decoded_packet.protocol() {
                Protocol::Tcp => return self.receive_tcp(decoded_packet, fd, payload_len),
                Protocol::Udp => {
                    if let Some(stream) = self.receive_udp(decoded_packet, payload_len)? {
                        return Ok(stream);
                    }
                }
            }
        }
    }

    fn receive_tcp(
        &self,
        decoded_packet: decoder::Packet,
        fd: Option<OwnedFd>,
        payload_len: usize,
    ) -> std::io::Result<Builder<Sub>> {
        let now = self.env.clock().get_time();

        let publisher = self.env.endpoint_publisher_with_time(now);

        let transfer_time = Self::get_transfer_time(decoded_packet.encode_time())?;

        let fd = tcp_fd(fd)?;
        let tcp_stream = Self::create_tcp_stream_from_fd(fd)?;

        let remote_address = tcp_stream.peer_addr()?;
        let mut buffer =
            Message::new_from_packet(decoded_packet.payload().to_vec(), remote_address);

        let initial_packet = match server::InitialPacket::peek(&mut buffer, 16) {
            Ok(packet) => packet,
            Err(err) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to peek initial packet, err: {err:?}"),
                ));
            }
        };

        let now = self.env.clock().get_time();
        let meta = event::api::ConnectionMeta {
            id: 0, // TODO use an actual connection ID
            timestamp: now.into_event(),
        };
        let info = event::api::ConnectionInfo {};
        let subscriber_ctx = self
            .env
            .subscriber()
            .create_connection_context(&meta, &info);

        let recv_buffer = recv::buffer::Local::new(buffer, None);
        let recv_buffer: either::Either<_, Channel> = Either::A(recv_buffer);
        let secret_control = vec![]; // this is only used while returning an error from endpoint::accept_stream

        let export_secret: ExportSecret =
            decoded_packet.export_secret().try_into().map_err(|e| {
                std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("Error parsing export secret {e:?}"),
                )
            })?;

        // create app pair
        let key_id = initial_packet.credentials.key_id;
        let initiator = Initiator::Remote;
        let secret = Secret::new(
            decoded_packet.ciphersuite(),
            decoded_packet.version_tag().into(),
            Type::Server,
            &export_secret,
        );
        let application = ApplicationPair::new(
            &secret,
            key_id,
            initiator,
            // Dedup should be done in manager
            Dedup::disabled(),
        );

        let control = None;
        let crypto = Bidirectional {
            credentials: initial_packet.credentials,
            application,
            control,
        };

        let local_port = tcp_stream.local_addr()?.port();
        let socket = LazyBoundStream::Tokio(tcp_stream);
        let peer = env::tcp::Reregistered {
            socket,
            peer_addr: remote_address.into(),
            local_port,
            recv_buffer,
        };

        // Reconstruct the forwarded application data, if any. Every failure is fail-open (the
        // stream is accepted with no application data) and published as a drop event so it is
        // counted by cause.
        let (application_data, drop_reason) =
            self.deserialize_application_data(decoded_packet.application_data());
        if let Some(reason) = drop_reason {
            publisher.on_acceptor_tcp_application_data_dropped(
                event::builder::AcceptorTcpApplicationDataDropped {
                    remote_address: &remote_address.into(),
                    reason,
                },
            );
        }

        let stream_builder = match endpoint::accept_stream(
            now,
            &self.env,
            peer,
            &initial_packet,
            &self.map,
            subscriber_ctx,
            None,
            crypto,
            decoded_packet.application_params().clone(),
            secret_control,
            application_data,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("Failed to accept stream, err: {:?}", error.error),
                ));
            }
        };

        {
            let remote_address: SocketAddress = stream_builder.shared.remote_addr();
            let remote_address = &remote_address;
            let creds = stream_builder.shared.credentials();
            let credential_id = &*creds.id;
            let stream_id = creds.key_id.as_u64();
            publisher.on_acceptor_tcp_socket_received(event::builder::AcceptorTcpSocketReceived {
                remote_address,
                credential_id,
                stream_id,
                transfer_time,
                payload_len,
            });
        }
        Ok(stream_builder)
    }

    /// Accepts a UDP stream handed off by a manager, or returns `None` if the packet belongs to a
    /// stream that was already handed off and has been routed to it.
    fn receive_udp(
        &self,
        decoded_packet: decoder::Packet,
        payload_len: usize,
    ) -> std::io::Result<Option<Builder<Sub>>> {
        let now = self.env.clock().get_time();

        let publisher = self.env.endpoint_publisher_with_time(now);

        let transfer_time = Self::get_transfer_time(decoded_packet.encode_time())?;

        let remote_address = *decoded_packet.peer_address().ok_or_else(|| {
            std::io::Error::new(ErrorKind::InvalidData, "UDP handoff is missing its peer")
        })?;

        self.check_local_address(decoded_packet.local_address())?;

        let initial_packet = Self::peek_initial_packet(decoded_packet.payload(), remote_address)?;
        let credentials = initial_packet.credentials;
        let segment = self.segment(remote_address, decoded_packet.payload())?;

        let Some(peer) = self.env.accept_handoff(&credentials, segment)? else {
            return Ok(None);
        };

        let meta = event::api::ConnectionMeta {
            id: 0, // TODO use an actual connection ID
            timestamp: now.into_event(),
        };
        let info = event::api::ConnectionInfo {};
        let subscriber_ctx = self
            .env
            .subscriber()
            .create_connection_context(&meta, &info);

        let secret = Self::secret(&decoded_packet)?;
        let key_id = credentials.key_id;
        let initiator = Initiator::Remote;
        let application = ApplicationPair::new(
            &secret,
            key_id,
            initiator,
            // Dedup should be done in manager
            Dedup::disabled(),
        );
        // UDP isn't reliable, so the stream needs control keys for its acknowledgements
        let (sealer, opener) = secret.control_pair(key_id, initiator);
        let crypto = Bidirectional {
            credentials,
            application,
            control: Some(ControlPair { sealer, opener }),
        };

        let (application_data, drop_reason) =
            self.deserialize_application_data(decoded_packet.application_data());
        if let Some(reason) = drop_reason {
            publisher.on_acceptor_udp_application_data_dropped(
                event::builder::AcceptorUdpApplicationDataDropped {
                    remote_address: &remote_address,
                    reason,
                },
            );
        }

        let stream_builder = endpoint::accept_stream(
            now,
            &self.env,
            peer,
            &initial_packet,
            &self.map,
            subscriber_ctx,
            None,
            crypto,
            decoded_packet.application_params().clone(),
            vec![],
            application_data,
        )
        .map_err(|error| {
            std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Failed to accept stream, err: {:?}", error.error),
            )
        })?;

        publisher.on_acceptor_udp_stream_received(event::builder::AcceptorUdpStreamReceived {
            remote_address: &remote_address,
            credential_id: &*credentials.id,
            stream_id: credentials.key_id.as_u64(),
            transfer_time,
        });
        tracing::trace!(payload_len, "received UDP handoff");

        Ok(Some(stream_builder))
    }

    /// Routes a packet the manager forwarded for an already handed-off UDP stream.
    ///
    /// Forwarded packets never create streams, so one that arrives for an unknown or finished
    /// stream is dropped.
    fn receive_forward(&self, packet_data: &mut [u8]) {
        let forward = match decoder::Forward::decode(DecoderBufferMut::new(packet_data)) {
            Ok((forward, _remaining)) => forward,
            Err(err) => {
                tracing::warn!(?err, "failed to decode forwarded packet");
                return;
            }
        };
        let remote_address = *forward.peer_address();
        let routed =
            Self::peek_initial_packet(forward.payload(), remote_address).and_then(|packet| {
                let segment = self.segment(remote_address, forward.payload())?;
                Ok(self.env.forward_handoff(&packet.credentials, segment))
            });
        match routed {
            Ok(true) => {}
            Ok(false) => tracing::debug!("dropping forwarded packet for an unknown stream"),
            Err(err) => tracing::debug!(?err, "dropping invalid forwarded packet"),
        }
    }

    fn check_local_address(&self, expected: Option<&SocketAddress>) -> std::io::Result<()> {
        let (Some(expected), Some(local)) = (expected, self.env.pool_addr()) else {
            return Ok(());
        };
        let expected = std::net::SocketAddr::from(*expected).ip();
        // The client only accepts replies from the IP it sent to, so a pool bound to a different
        // IP would never complete the stream.
        if local.ip().is_unspecified() || local.ip() == expected {
            return Ok(());
        }
        Err(std::io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "UDP handoff for {expected} but the receive pool is bound to {}",
                local.ip()
            ),
        ))
    }

    fn peek_initial_packet(
        payload: &[u8],
        remote_address: SocketAddress,
    ) -> std::io::Result<server::InitialPacket> {
        let mut buffer = Message::new_from_packet(payload.to_vec(), remote_address.into());
        server::InitialPacket::peek(&mut buffer, 16).map_err(|err| {
            std::io::Error::new(
                ErrorKind::InvalidData,
                format!("Failed to peek initial packet, err: {err:?}"),
            )
        })
    }

    /// Copies a handed-off packet into a receive descriptor, as if the pool's socket received it.
    fn segment(
        &self,
        remote_address: SocketAddress,
        payload: &[u8],
    ) -> std::io::Result<descriptor::Filled> {
        let unfilled = self
            .packets
            .lock()
            .map_err(|_| std::io::Error::other("receive descriptor pool lock poisoned"))?
            .alloc_or_grow();
        let mut segments = unfilled
            .recv_with(|addr, _cmsg, mut buffer| {
                let Some(buffer) = buffer.get_mut(..payload.len()) else {
                    return Err(std::io::Error::new(
                        ErrorKind::InvalidData,
                        "handed-off packet exceeds the receive buffer",
                    ));
                };
                buffer.copy_from_slice(payload);
                addr.set(remote_address);
                Ok(payload.len())
            })
            .map_err(|(_unfilled, err)| err)?;
        segments
            .next()
            .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, "empty handed-off packet"))
    }

    fn secret(decoded_packet: &decoder::Packet) -> std::io::Result<Secret> {
        let export_secret: ExportSecret =
            decoded_packet.export_secret().try_into().map_err(|e| {
                std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("Error parsing export secret {e:?}"),
                )
            })?;
        Ok(Secret::new(
            decoded_packet.ciphersuite(),
            decoded_packet.version_tag().into(),
            Type::Server,
            &export_secret,
        ))
    }

    /// Reconstructs the forwarded application data, if any. Every failure is fail-open (the
    /// stream is accepted with no application data) and the returned reason is published as a
    /// drop event so it is counted by cause.
    fn deserialize_application_data(
        &self,
        blob: Option<&[u8]>,
    ) -> (
        Option<ApplicationData>,
        Option<event::builder::AcceptorTcpApplicationDataDropReason>,
    ) {
        use event::builder::AcceptorTcpApplicationDataDropReason as Reason;
        match (blob, self.application_data_deserializer.as_ref()) {
            // No blob on the wire: nothing to attach (v0 packet, or nothing to forward).
            (None, _) => (None, None),
            (Some(blob), Some(deserializer)) => match deserializer(blob) {
                Ok(application_data) => (application_data, None),
                Err(err) => {
                    tracing::warn!(?err, "failed to deserialize application data");
                    (None, Some(Reason::DeserializeFailed))
                }
            },
            // A blob arrived but this server has no deserializer registered. There is no error
            // text to log and a misconfigured server would repeat it on every stream, so the
            // counter is the only signal.
            (Some(_), None) => (None, Some(Reason::NoDeserializer)),
        }
    }

    fn decode_packet(packet: &mut [u8]) -> std::io::Result<decoder::Packet> {
        let decoder_buffer = DecoderBufferMut::new(packet);

        match decoder::Packet::decode(decoder_buffer) {
            Ok((packet, remaining)) => {
                if !remaining.is_empty() {
                    tracing::warn!("Buffer not empty after decoding packet");
                }
                Ok(packet)
            }
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to decode unix packet: {e:?}"),
            )),
        }
    }

    fn get_transfer_time(encode_time: u64) -> std::io::Result<Duration> {
        #[cfg(target_os = "linux")]
        let clock = ClockId::CLOCK_MONOTONIC_RAW;

        #[cfg(not(target_os = "linux"))]
        let clock = ClockId::CLOCK_MONOTONIC;

        let now = clock.now().map_err(io::Error::from)?;
        let transfer_time = now.num_microseconds() as u64 - encode_time;
        Ok(Duration::from_micros(transfer_time))
    }

    fn create_tcp_stream_from_fd(fd: OwnedFd) -> std::io::Result<tokio::net::TcpStream> {
        let std_stream = std::net::TcpStream::from(fd);
        std_stream.set_nonblocking(true)?;
        TcpStream::from_std(std_stream)
    }
}

/// A map for streams accepted from a manager, which holds the real path secrets.
pub fn placeholder_map<Sub: Subscriber + Clone>(subscriber: Sub) -> Map {
    Map::new(
        stateless_reset::Signer::random(),
        1,
        false,
        time::NoopClock,
        subscriber,
    )
}

fn tcp_fd(fd: Option<OwnedFd>) -> std::io::Result<OwnedFd> {
    fd.ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidData,
            "TCP handoff is missing its socket file descriptor",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_handoff_requires_a_file_descriptor() {
        let err = tcp_fd(None).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);

        let fd = OwnedFd::from(std::fs::File::open("/dev/null").unwrap());
        assert!(tcp_fd(Some(fd)).is_ok());
    }
}

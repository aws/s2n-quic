// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    either::{self, Either},
    event::{self, EndpointPublisher, Subscriber},
    msg::recv::Message,
    packet::{
        self,
        uds::{decoder, Protocol},
    },
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
            tokio::{self as env, pool::PacketKind, Environment},
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

    /// Creates a receiver with the map used by `env`'s receive pool.
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
        let secret_control = vec![];
        let secret = Self::secret(&decoded_packet)?;

        let key_id = initial_packet.credentials.key_id;
        let initiator = Initiator::Remote;
        let application = ApplicationPair::new(
            &secret,
            key_id,
            initiator,
            // The manager already claimed this key.
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

    /// Accepts a new handoff, or routes to an existing stream and returns `None`.
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
            // The manager already claimed this key.
            Dedup::disabled(),
        );
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

    /// Routes packet-only forwards without allocating streams.
    fn receive_forward(&self, packet_data: &mut [u8]) {
        let forward = match Self::decode_forward(packet_data) {
            Ok(forward) => forward,
            Err(err) => {
                tracing::warn!(?err, "failed to decode forwarded packet");
                return;
            }
        };
        let remote_address = *forward.peer_address();
        let routed = self
            .segment(remote_address, forward.payload())
            .and_then(|mut segment| {
                let (credentials, kind) = Self::forwarded_packet(segment.payload_mut())?;
                Ok(self.env.forward_handoff(&credentials, kind, segment))
            });
        match routed {
            Ok(true) => {}
            Ok(false) => tracing::debug!("dropping forwarded packet for an unknown stream"),
            Err(err) => tracing::debug!(?err, "dropping invalid forwarded packet"),
        }
    }

    fn decode_forward(packet_data: &mut [u8]) -> io::Result<decoder::Forward> {
        let (forward, remaining) = decoder::Forward::decode(DecoderBufferMut::new(packet_data))
            .map_err(|err| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("Failed to decode forwarded UDS packet: {err:?}"),
                )
            })?;
        if !remaining.is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "Forwarded UDS packet has trailing data",
            ));
        }
        Ok(forward)
    }

    fn forwarded_packet(payload: &mut [u8]) -> io::Result<(Credentials, PacketKind)> {
        let (packet, remaining) = DecoderBufferMut::new(payload)
            .decode_parameterized::<packet::Packet>(16)
            .map_err(|err| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("Failed to decode forwarded packet: {err:?}"),
                )
            })?;
        if !remaining.is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "Forwarded packet has trailing data",
            ));
        }
        match packet {
            packet::Packet::Stream(packet) => Ok((*packet.credentials(), PacketKind::Stream)),
            packet::Packet::Control(packet) => Ok((*packet.credentials(), PacketKind::Control)),
            _ => Err(io::Error::new(
                ErrorKind::InvalidData,
                "Unexpected forwarded packet type",
            )),
        }
    }

    fn check_local_address(&self, expected: Option<&SocketAddress>) -> std::io::Result<()> {
        let (Some(expected), Some(local)) = (expected, self.env.pool_addr()) else {
            return Ok(());
        };
        let expected = std::net::SocketAddr::from(*expected).ip();
        // Clients accept replies only from the IP they contacted.
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

    /// Copies packet bytes into a receive descriptor.
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

/// Creates a placeholder map; the manager retains the path secrets.
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
    use crate::testing::NoopSubscriber;
    use s2n_codec::{Encoder, EncoderBuffer, EncoderLenEstimator};
    use s2n_quic_core::varint::VarInt;

    type TestReceiver = Receiver<NoopSubscriber>;

    fn credentials() -> Credentials {
        Credentials {
            id: Default::default(),
            key_id: 7u8.into(),
        }
    }

    // Authentication stays with the stream; these tests exercise parsing before that step.
    fn packet(kind: PacketKind, queue_id: VarInt) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        let mut encoder = EncoderBuffer::new(&mut bytes);
        match kind {
            PacketKind::Stream => encoder.encode(&packet::stream::Tag::default()),
            PacketKind::Control => {
                let mut tag = packet::control::Tag::default();
                tag.set_is_stream(true);
                encoder.encode(&tag);
            }
        }
        encoder.encode(&credentials());
        encoder.encode(&packet::WireVersion::ZERO);
        if kind == PacketKind::Stream {
            encoder.encode(&0u16);
        }
        encoder.encode(&packet::stream::Id::normal(queue_id).unwrap());
        encoder.encode(&VarInt::ZERO);
        if kind == PacketKind::Stream {
            encoder.encode(&0u32);
            encoder.encode(&VarInt::ZERO);
            encoder.encode(&VarInt::ZERO);
        }
        encoder.encode(&VarInt::ZERO);
        encoder.write_repeated(16, 0);
        let len = encoder.len();
        bytes.truncate(len);
        bytes
    }

    fn forward(payload: &[u8]) -> Vec<u8> {
        let peer: SocketAddress = "127.0.0.1:1234"
            .parse::<std::net::SocketAddr>()
            .unwrap()
            .into();
        let len = packet::uds::encoder::encode_forward(
            &mut EncoderLenEstimator::new(usize::MAX),
            0,
            &peer,
            payload,
        );
        let mut bytes = vec![0; len];
        packet::uds::encoder::encode_forward(
            &mut EncoderBuffer::new(&mut bytes),
            0,
            &peer,
            payload,
        );
        bytes
    }

    #[test]
    fn packet_only_forwards_decode_stream_and_control_with_any_queue_id() {
        for queue_id in [VarInt::ZERO, 99u8.into()] {
            for kind in [PacketKind::Stream, PacketKind::Control] {
                let expected = packet(kind, queue_id);
                let mut envelope = forward(&expected);
                let decoded = TestReceiver::decode_forward(&mut envelope).unwrap();
                let mut payload = decoded.payload().to_vec();
                assert_eq!(
                    TestReceiver::forwarded_packet(&mut payload).unwrap(),
                    (credentials(), kind)
                );
                assert_eq!(payload, expected, "forwarded ciphertext must remain intact");
            }
        }
    }

    #[test]
    fn packet_only_forwards_reject_trailing_data() {
        let mut bytes = packet(PacketKind::Stream, VarInt::ZERO);
        bytes.push(0);
        assert_eq!(
            TestReceiver::forwarded_packet(&mut bytes)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );

        let mut envelope = forward(&packet(PacketKind::Control, VarInt::ZERO));
        envelope.push(0);
        assert_eq!(
            TestReceiver::decode_forward(&mut envelope)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
    }

    #[test]
    fn packet_only_forwards_reject_datagrams_and_truncated_packets() {
        let mut datagram = vec![0; 128];
        let mut encoder = EncoderBuffer::new(&mut datagram);
        encoder.encode(&packet::datagram::Tag::default());
        encoder.encode(&credentials());
        encoder.encode(&packet::WireVersion::ZERO);
        encoder.encode(&0u16);
        encoder.encode(&VarInt::ZERO);
        encoder.write_repeated(16, 0);
        let len = encoder.len();
        datagram.truncate(len);
        assert_eq!(
            TestReceiver::forwarded_packet(&mut datagram)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );

        for kind in [PacketKind::Stream, PacketKind::Control] {
            let bytes = packet(kind, VarInt::ZERO);
            for len in 0..bytes.len() {
                let mut truncated = bytes[..len].to_vec();
                assert_eq!(
                    TestReceiver::forwarded_packet(&mut truncated)
                        .unwrap_err()
                        .kind(),
                    ErrorKind::InvalidData
                );
            }
        }
    }

    #[test]
    fn tcp_handoff_requires_a_file_descriptor() {
        let err = tcp_fd(None).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);

        let fd = OwnedFd::from(std::fs::File::open("/dev/null").unwrap());
        assert!(tcp_fd(Some(fd)).is_ok());
    }
}

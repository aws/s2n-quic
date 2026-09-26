// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    either::{self, Either},
    event::{self, EndpointPublisher, Subscriber},
    msg::recv::Message,
    packet::uds::decoder,
    path::secret::{
        map::{ApplicationData, ApplicationDataError, ApplicationPair, Bidirectional, Dedup},
        schedule::{ExportSecret, Initiator, Secret},
        stateless_reset, Map,
    },
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
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpStream;

/// Reconstructs opaque application-data bytes carried in a UDS handoff packet back into the
/// type-erased [`ApplicationData`] attached to accepted streams.
///
/// The dc crate never inspects the bytes; the callback is the application's inverse of the
/// serializer registered on the forwarding side. Returning `Ok(None)` or `Err` results in the
/// stream being accepted with no application data (fail-open).
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
}

impl<Sub> Receiver<Sub>
where
    Sub: Subscriber + Clone,
{
    pub fn new(socket_path: &Path, env: &Environment<Sub>) -> std::io::Result<Self> {
        let receiver = uds::receiver::Receiver::new(socket_path)?;
        let sub = env.subscriber();
        let map = Map::new(
            stateless_reset::Signer::random(),
            1,
            false,
            time::NoopClock,
            sub.clone(),
        );
        Ok(Self {
            receiver,
            env: env.clone(),
            map,
            application_data_deserializer: None,
        })
    }

    /// Registers the callback used to reconstruct [`ApplicationData`] from the opaque blob carried
    /// in a UDS handoff packet. When unset, accepted streams carry no application data.
    pub fn with_application_data_deserializer(
        mut self,
        deserializer: ApplicationDataDeserializer,
    ) -> Self {
        self.application_data_deserializer = Some(deserializer);
        self
    }

    pub async fn receive_stream(&self) -> std::io::Result<Builder<Sub>> {
        let now = self.env.clock().get_time();

        let publisher = self.env.endpoint_publisher_with_time(now);

        let (mut packet_data, fd) = self.receiver.receive_msg().await?;

        let decoded_packet = Self::decode_packet(&mut packet_data)?;

        let transfer_time = Self::get_transfer_time(decoded_packet.encode_time())?;

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
        let application_data = match (
            decoded_packet.application_data(),
            self.application_data_deserializer.as_ref(),
        ) {
            // No blob on the wire: nothing to attach (v0 packet, or nothing to forward).
            (None, _) => None,
            (Some(blob), Some(deserializer)) => match deserializer(blob) {
                Ok(application_data) => application_data,
                Err(err) => {
                    publisher.on_acceptor_tcp_application_data_dropped(
                        event::builder::AcceptorTcpApplicationDataDropped {
                            remote_address: &remote_address.into(),
                            reason: event::builder::AcceptorTcpApplicationDataDropReason::DeserializeFailed,
                        },
                    );
                    tracing::warn!(?err, "failed to deserialize application data");
                    None
                }
            },
            // A blob arrived but this server has no deserializer registered. There is no error
            // text to log and a misconfigured server would repeat it on every stream, so the
            // counter is the only signal.
            (Some(_), None) => {
                publisher.on_acceptor_tcp_application_data_dropped(
                    event::builder::AcceptorTcpApplicationDataDropped {
                        remote_address: &remote_address.into(),
                        reason:
                            event::builder::AcceptorTcpApplicationDataDropReason::NoDeserializer,
                    },
                );
                None
            }
        };

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
                payload_len: packet_data.len(),
            });
        }
        Ok(stream_builder)
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

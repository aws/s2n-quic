// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    either::{self, Either},
    event::{self, EndpointPublisher, Subscriber},
    msg::recv::Message,
    packet::uds::decoder,
    path::secret::{
        map::{ApplicationPair, Bidirectional, Dedup},
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
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    endpoint::Type,
    event::IntoEvent as _,
    inet::SocketAddress,
    time::{self, Clock as _},
};
use std::{io::ErrorKind, os::fd::OwnedFd, path::Path, time::Duration};
use tokio::net::TcpStream;

pub struct Receiver<Sub>
where
    Sub: Subscriber + Clone,
{
    receiver: uds::receiver::Receiver,
    env: Environment<Sub>,
}

impl<Sub> Receiver<Sub>
where
    Sub: Subscriber + Clone,
{
    pub fn new(socket_path: &Path, env: &Environment<Sub>) -> std::io::Result<Self> {
        let receiver = uds::receiver::Receiver::new(socket_path)?;
        Ok(Self {
            receiver,

            env: env.clone(),
        })
    }

    pub async fn receive_stream(&self) -> std::io::Result<Builder<Sub>> {
        let now = self.env.clock().get_time();

        let publisher = self.env.endpoint_publisher_with_time(now);

        let (packet_data, fd) = self.receiver.receive_msg().await?;

        let decoded_packet = self.decode_packet(&packet_data)?;

        let tcp_stream = self.create_tcp_stream_from_fd(fd)?;

        let remote_address = tcp_stream.peer_addr()?;
        let mut buffer =
            Message::new_from_packet(decoded_packet.payload().to_vec().clone(), remote_address);

        let initial_packet = match server::InitialPacket::peek(&mut buffer, 16) {
            Ok(packet) => packet,
            Err(err) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to peek initial packet, err: {:?}", err),
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
        let sub = self.env.subscriber();

        let map = Map::new(
            stateless_reset::Signer::random(),
            1,
            time::NoopClock,
            sub.clone(),
        );
        let secret_control = vec![];

        let export_secret: ExportSecret =
            decoded_packet.export_secret().try_into().map_err(|e| {
                std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("Error parsing export secret {:?}", e),
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
        let stream_builder = match endpoint::accept_stream(
            now,
            &self.env,
            peer,
            &initial_packet,
            &map,
            subscriber_ctx,
            None,
            crypto,
            decoded_packet.application_params().clone(),
            secret_control,
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
            publisher.on_acceptor_tcp_stream_enqueued(event::builder::AcceptorTcpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time: Duration::new(0, 0),
                blocked_count: 0,
            });
        }
        Ok(stream_builder)
    }

    fn decode_packet(&self, data: &[u8]) -> std::io::Result<decoder::Packet> {
        let mut buffer = data.to_vec();
        let decoder_buffer = DecoderBufferMut::new(&mut buffer);

        match decoder::Packet::decode(decoder_buffer) {
            Ok((packet, remaining)) => {
                if !remaining.is_empty() {
                    tracing::warn!("Buffer not empty after decoding packet");
                }
                Ok(packet)
            }
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to decode unix packet: {:?}", e),
            )),
        }
    }

    fn create_tcp_stream_from_fd(&self, fd: OwnedFd) -> std::io::Result<tokio::net::TcpStream> {
        let std_stream = std::net::TcpStream::from(fd);
        std_stream.set_nonblocking(true)?;
        TcpStream::from_std(std_stream)
    }
}

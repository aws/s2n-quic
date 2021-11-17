// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    file::File,
    interop::{self, Testcase},
    Result,
};
use core::convert::TryInto;
use futures::stream::StreamExt;
use s2n_quic::{
    provider::{
        endpoint_limits,
        event::{events, Subscriber},
        io,
        tls::default::certificate::{Certificate, IntoCertificate, IntoPrivateKey, PrivateKey},
    },
    stream::BidirectionalStream,
    Connection, Server,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use structopt::StructOpt;
use tokio::spawn;
use tracing::debug;

#[derive(Debug, StructOpt)]
pub struct Interop {
    #[structopt(short, long, default_value = "::")]
    ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(long)]
    certificate: Option<PathBuf>,

    #[structopt(long)]
    private_key: Option<PathBuf>,

    #[structopt(long, default_value = "hq-interop")]
    alpn_protocols: Vec<String>,

    #[structopt(long, default_value = ".")]
    www_dir: PathBuf,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(long, env = "TESTCASE", possible_values = &Testcase::supported(is_supported_testcase))]
    testcase: Option<Testcase>,
}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        let mut server = self.server()?;

        let www_dir: Arc<Path> = Arc::from(self.www_dir.as_path());

        while let Some(connection) = server.accept().await {
            let unspecified: std::net::SocketAddr = ([0, 0, 0, 0], 0).into();
            println!(
                "Accepted a QUIC connection from {} on {}",
                connection.remote_addr().unwrap_or(unspecified),
                connection.local_addr().unwrap_or(unspecified)
            );

            // TODO check the ALPN of the connection to determine handler

            // spawn a task per connection
            spawn(handle_h09_connection(connection, www_dir.clone()));
        }

        async fn handle_h09_connection(mut connection: Connection, www_dir: Arc<Path>) {
            loop {
                match connection.accept_bidirectional_stream().await {
                    Ok(Some(stream)) => {
                        let _ = connection.query_event_context_mut(
                            |context: &mut MyConnectionContext| context.stream_requests += 1,
                        );

                        let www_dir = www_dir.clone();
                        // spawn a task per stream
                        tokio::spawn(async move {
                            if let Err(err) = handle_h09_stream(stream, www_dir).await {
                                eprintln!("Stream errror: {:?}", err)
                            }
                        });
                    }
                    Ok(None) => {
                        // the connection was closed without an error
                        let context = connection
                            .query_event_context(|context: &MyConnectionContext| *context)
                            .expect("query should execute");
                        debug!("Final stats: {:?}", context);
                        return;
                    }
                    Err(err) => {
                        eprintln!("error while accepting stream: {}", err);
                        let context = connection
                            .query_event_context(|context: &MyConnectionContext| *context)
                            .expect("query should execute");
                        debug!("Final stats: {:?}", context);
                        return;
                    }
                }
            }
        }

        async fn handle_h09_stream(stream: BidirectionalStream, www_dir: Arc<Path>) -> Result<()> {
            let (rx_stream, mut tx_stream) = stream.split();
            let path = interop::read_request(rx_stream).await?;
            let mut abs_path = www_dir.to_path_buf();
            abs_path.extend(
                path.split('/')
                    .filter(|segment| !segment.starts_with('.'))
                    .map(std::path::Path::new),
            );
            let mut file = File::open(&abs_path).await?;
            loop {
                match file.next().await {
                    Some(Ok(chunk)) => tx_stream.send(chunk).await?,
                    Some(Err(err)) => {
                        eprintln!("error opening {:?}", abs_path);
                        tx_stream.reset(1u32.try_into()?)?;
                        return Err(err.into());
                    }
                    None => {
                        tx_stream.finish()?;
                        return Ok(());
                    }
                }
            }
        }

        Err(crate::CRASH_ERROR_MESSAGE.into())
    }

    fn server(&self) -> Result<Server> {
        let private_key = self.private_key()?;
        let certificate = self.certificate()?;

        // The server builder defaults to a chain because this allows certs to just work, whether
        // the PEM contains a single cert or a chain
        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(certificate, private_key)?
            .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
            .with_key_logging()?
            .build()?;

        let mut max_handshakes = 100;
        if let Some(Testcase::Retry) = self.testcase {
            max_handshakes = 0;
        }

        let limits = endpoint_limits::Default::builder()
            .with_inflight_handshake_limit(max_handshakes)?
            .build()?;

        let mut io_builder =
            io::Default::builder().with_receive_address((self.ip, self.port).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let server = Server::builder()
            .with_io(io)?
            .with_tls(tls)?
            .with_endpoint_limits(limits)?
            .with_event(EventSubscriber(1))?
            .start()
            .unwrap();

        eprintln!("Server listening on port {}", self.port);

        Ok(server)
    }

    fn certificate(&self) -> Result<Certificate> {
        Ok(if let Some(pathbuf) = self.certificate.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }

    fn private_key(&self) -> Result<PrivateKey> {
        Ok(if let Some(pathbuf) = self.private_key.as_ref() {
            pathbuf.into_private_key()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
        })
    }
}

fn is_supported_testcase(testcase: Testcase) -> bool {
    use Testcase::*;
    match testcase {
        VersionNegotiation => true,
        Handshake => true,
        Transfer => true,
        ChaCha20 => true,
        // KeyUpdate is client only
        KeyUpdate => false,
        Retry => true,
        // TODO support issuing tickets
        Resumption => false,
        // TODO implement 0rtt
        ZeroRtt => false,
        // TODO integrate a H3 implementation
        Http3 => false,
        Multiconnect => true,
        Ecn => true,
        ConnectionMigration => true,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MyConnectionContext {
    packet_sent: u64,
    stream_requests: u64,
}

pub struct EventSubscriber(usize);

impl Subscriber for EventSubscriber {
    type ConnectionContext = MyConnectionContext;

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
        MyConnectionContext {
            packet_sent: 0,
            stream_requests: 0,
        }
    }

    fn on_active_path_updated(
        &mut self,
        _context: &mut Self::ConnectionContext,
        meta: &events::ConnectionMeta,
        event: &events::ActivePathUpdated,
    ) {
        debug!("{:?} {:?}", meta.id, event);
    }

    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &events::ConnectionMeta,
        _event: &events::PacketSent,
    ) {
        context.packet_sent += 1;
    }

    fn on_platform_feature_configured(
        &mut self,
        _meta: &events::EndpointMeta,
        event: &events::PlatformFeatureConfigured,
    ) {
        debug!("{:?}", event.configuration)
    }

    fn on_frame_sent(
        &mut self,
        _context: &mut Self::ConnectionContext,
        meta: &events::ConnectionMeta,
        event: &events::FrameSent,
    ) {
        if let events::EndpointType::Server { .. } = meta.endpoint_type {
            if let events::Frame::HandshakeDone { .. } = event.frame {
                debug!("{:?} Handshake is complete and confirmed at Server! HANDSHAKE_DONE frame was sent", meta)
            }
        }
    }

    fn on_frame_received(
        &mut self,
        _context: &mut Self::ConnectionContext,
        meta: &events::ConnectionMeta,
        event: &events::FrameReceived,
    ) {
        if let events::EndpointType::Client { .. } = meta.endpoint_type {
            if let events::Frame::HandshakeDone { .. } = event.frame {
                debug!(
                    "{:?} Handshake is confirmed at Client. HANDSHAKE_DONE frame was received",
                    meta
                )
            }
        }
    }

    fn on_connection_migration_denied(
        &mut self,
        _context: &mut Self::ConnectionContext,
        meta: &events::ConnectionMeta,
        event: &events::ConnectionMigrationDenied,
    ) {
        debug!("{:?} {:?}", meta.id, event);
    }

    fn on_handshake_status_updated(
        &mut self,
        _context: &mut Self::ConnectionContext,
        meta: &events::ConnectionMeta,
        event: &events::HandshakeStatusUpdated,
    ) {
        debug!("{:?} {:?}", meta.id, event);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    file::{abs_path, File},
    interop::{self, Testcase},
    server::h3,
    tls,
    tls::TlsProviders,
    Result,
};
use core::convert::TryInto;
use futures::stream::StreamExt;
use s2n_quic::{
    provider::{
        endpoint_limits,
        event::{events, Subscriber},
        io,
    },
    stream::BidirectionalStream,
    Connection, Server,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use structopt::StructOpt;
use tokio::{spawn, time::timeout};
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
    application_protocols: Vec<String>,

    #[structopt(long, default_value = ".")]
    www_dir: PathBuf,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(long, env = "TESTCASE", possible_values = &Testcase::supported(is_supported_testcase))]
    testcase: Option<Testcase>,

    #[structopt(long, default_value)]
    tls: TlsProviders,
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

            // spawn a task per connection
            match &(connection.application_protocol()?)[..] {
                b"h3" => spawn(h3::handle_connection(connection, www_dir.clone())),
                b"hq-interop" => spawn(handle_h09_connection(connection, www_dir.clone())),
                _ => spawn(async move {
                    eprintln!(
                        "Unsupported application protocol: {:?}",
                        connection.application_protocol()
                    );
                }),
            };
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
            let abs_path = abs_path(&path, &www_dir);
            let mut file = File::open(&abs_path).await?;
            loop {
                match timeout(Duration::from_secs(1), file.next()).await {
                    Ok(Some(Ok(chunk))) => {
                        let len = chunk.len();
                        debug!(
                            "{:?} bytes ready to send on Stream({:?})",
                            len,
                            tx_stream.id()
                        );
                        tx_stream.send(chunk).await?;
                        debug!("{:?} bytes sent on Stream({:?})", len, tx_stream.id());
                    }
                    Ok(Some(Err(err))) => {
                        eprintln!("error opening {:?}", abs_path);
                        tx_stream.reset(1u32.try_into()?)?;
                        return Err(err.into());
                    }
                    Ok(None) => {
                        tx_stream.finish()?;
                        return Ok(());
                    }
                    Err(_) => {
                        eprintln!("timeout opening {:?}", abs_path);
                    }
                }
            }
        }

        Err(crate::CRASH_ERROR_MESSAGE.into())
    }

    fn server(&self) -> Result<Server> {
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
            .with_endpoint_limits(limits)?
            .with_event((
                EventSubscriber(1),
                s2n_quic::provider::event::tracing::Subscriber::default(),
            ))?;
        let server = match self.tls {
            #[cfg(unix)]
            TlsProviders::S2N => {
                // The server builder defaults to a chain because this allows certs to just work, whether
                // the PEM contains a single cert or a chain
                let tls = s2n_quic::provider::tls::s2n_tls::Server::builder()
                    .with_certificate(
                        tls::s2n::ca(self.certificate.as_ref())?,
                        tls::s2n::private_key(self.private_key.as_ref())?,
                    )?
                    .with_application_protocols(
                        self.application_protocols.iter().map(String::as_bytes),
                    )?
                    .with_key_logging()?
                    .build()?;

                server.with_tls(tls)?.start().unwrap()
            }
            TlsProviders::Rustls => {
                // The server builder defaults to a chain because this allows certs to just work, whether
                // the PEM contains a single cert or a chain
                let tls = s2n_quic::provider::tls::rustls::Server::builder()
                    .with_certificate(
                        tls::rustls::ca(self.certificate.as_ref())?,
                        tls::rustls::private_key(self.private_key.as_ref())?,
                    )?
                    .with_application_protocols(
                        self.application_protocols.iter().map(String::as_bytes),
                    )?
                    .with_key_logging()?
                    .build()?;

                server.with_tls(tls)?.start().unwrap()
            }
        };

        eprintln!("Server listening on port {}", self.port);

        Ok(server)
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
        Http3 => true,
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

    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &events::ConnectionMeta,
        _event: &events::PacketSent,
    ) {
        context.packet_sent += 1;
    }
}

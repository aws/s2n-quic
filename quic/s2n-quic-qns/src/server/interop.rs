// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    interop::Testcase,
    server,
    server::{h09, h3},
    tls, Result,
};
use s2n_quic::{
    provider::{
        endpoint_limits,
        event::{events, Subscriber},
    },
    Server,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use structopt::StructOpt;
use tokio::spawn;

#[derive(Debug, StructOpt)]
pub struct Interop {
    #[structopt(long, default_value = "hq-interop")]
    application_protocols: Vec<String>,

    #[structopt(long, default_value = ".")]
    www_dir: PathBuf,

    #[structopt(long, env = "TESTCASE", possible_values = &Testcase::supported(is_supported_testcase))]
    testcase: Option<Testcase>,

    #[structopt(flatten)]
    limits: crate::limits::Limits,

    #[structopt(flatten)]
    tls: tls::Server,

    #[structopt(flatten)]
    io: crate::io::Server,

    #[structopt(flatten)]
    runtime: crate::runtime::Runtime,

    #[structopt(flatten)]
    congestion_controller: crate::congestion_control::CongestionControl,
}

impl Interop {
    pub fn run(&self) -> Result<()> {
        self.runtime.build()?.block_on(self.task())
    }

    async fn task(&self) -> Result<()> {
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
                b"hq-interop" => spawn(h09::handle_connection(connection, www_dir.clone())),
                _ => spawn(async move {
                    eprintln!(
                        "Unsupported application protocol: {:?}",
                        connection.application_protocol()
                    );
                }),
            };
        }

        Err(crate::CRASH_ERROR_MESSAGE.into())
    }

    fn server(&self) -> Result<Server> {
        let mut max_handshakes = 100;
        if let Some(Testcase::Retry) = self.testcase {
            max_handshakes = 0;
        }

        let endpoint_limits = endpoint_limits::Default::builder()
            .with_inflight_handshake_limit(max_handshakes)?
            .build()?;

        let limits = self.limits.limits();

        let io = self.io.build()?;

        let server = Server::builder()
            .with_io(io)?
            .with_endpoint_limits(endpoint_limits)?
            .with_limits(limits)?
            .with_event((
                EventSubscriber(1),
                s2n_quic::provider::event::tracing::Subscriber::default(),
            ))?;
        let server = server::build(
            server,
            &self.application_protocols,
            &self.tls,
            &self.congestion_controller,
        )?;

        eprintln!("Server listening on port {}", self.io.port);

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
    pub(crate) stream_requests: u64,
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

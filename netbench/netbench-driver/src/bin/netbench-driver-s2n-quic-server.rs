// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{scenario, timer::Timestamp, Result, Timer};
use netbench_driver::Allocator;
use s2n_quic::{provider::io, Connection};
use std::{collections::HashSet, sync::Arc};
use structopt::StructOpt;
use tokio::spawn;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    Server::from_args().run().await
}

#[derive(Debug, StructOpt)]
pub struct Server {
    #[structopt(flatten)]
    opts: netbench_driver::Server,

    #[structopt(long, default_value = "9001", env = "MAX_MTU")]
    max_mtu: u16,

    #[structopt(long, env = "DISABLE_GSO")]
    disable_gso: bool,
}

impl Server {
    pub async fn run(&self) -> Result<()> {
        let scenario = self.opts.scenario();
        let trace = self.opts.trace();

        let mut server = self.server(trace.clone())?;

        while let Some(connection) = server.accept().await {
            let scenario = scenario.clone();
            let trace = trace.clone();
            spawn(async move {
                if let Err(error) = handle_connection(connection, scenario, trace).await {
                    eprintln!("error: {error:#}");
                }
            });
        }

        return Err("server shut down unexpectedly".into());

        async fn handle_connection(
            connection: Connection,
            scenario: Arc<scenario::Server>,
            mut trace: impl netbench::Trace,
        ) -> Result<()> {
            let server_name = connection.server_name()?.ok_or("missing server name")?;
            let scenario = scenario.on_server_name(&server_name)?;
            let conn =
                netbench::Driver::new(scenario, netbench::s2n_quic::Connection::new(connection));

            let mut checkpoints = HashSet::new();
            let mut timer = netbench::timer::Tokio::default();

            conn.run(&mut trace, &mut checkpoints, &mut timer).await?;

            Ok(())
        }
    }

    fn server(&self, trace: impl netbench::Trace + Send + 'static) -> Result<s2n_quic::Server> {
        let (certificate, private_key) = self.opts.certificate();
        let certificate = certificate.pem.as_str();
        let private_key = private_key.pem.as_str();

        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(certificate, private_key)?
            .with_application_protocols(
                self.opts.application_protocols.iter().map(String::as_bytes),
            )?
            .with_key_logging()?
            .build()?;

        let mut io_builder = io::Default::builder()
            .with_max_mtu(self.max_mtu)?
            .with_receive_address((self.opts.ip, self.opts.port).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let server = s2n_quic::Server::builder()
            .with_io(io)?
            .with_tls(tls)?
            .with_event(EventTracer::new(trace))?
            .start()
            .unwrap();

        Ok(server)
    }
}

struct EventTracer<T> {
    trace: T,
    timer: netbench::timer::Tokio,
}

impl<T> EventTracer<T> {
    fn new(trace: T) -> Self {
        Self {
            trace,
            timer: Default::default(),
        }
    }
}

impl<T: 'static + Send + netbench::Trace> s2n_quic::provider::event::Subscriber for EventTracer<T> {
    type ConnectionContext = Timestamp;

    #[inline]
    fn create_connection_context(
        &mut self,
        _meta: &s2n_quic::provider::event::events::ConnectionMeta,
        _info: &s2n_quic::provider::event::ConnectionInfo,
    ) -> Timestamp {
        self.timer.now()
    }

    #[inline]
    fn on_handshake_status_updated(
        &mut self,
        start: &mut Timestamp,
        meta: &s2n_quic::provider::event::events::ConnectionMeta,
        event: &s2n_quic::provider::event::events::HandshakeStatusUpdated,
    ) {
        use s2n_quic::provider::event::events::HandshakeStatus;

        // record the difference of when we started the connection and completed the handshake
        if let HandshakeStatus::Complete { .. } = event.status {
            let now = self.timer.now();
            self.trace.connect(now, meta.id, now - *start)
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{multiplex, scenario, Result};
use netbench_driver::Allocator;
use s2n_tls::raw::{
    config::{Builder, Config},
    error::Error,
    security::DEFAULT_TLS13,
};
use s2n_tls_tokio::TlsAcceptor;
use std::{collections::HashSet, sync::Arc};
use structopt::StructOpt;
use tokio::{
    net::{TcpListener, TcpStream},
    spawn,
};

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
}

impl Server {
    pub async fn run(&self) -> Result<()> {
        let scenario = self.opts.scenario();

        let server = self.server().await?;

        let trace = self.opts.trace();
        let acceptor = TlsAcceptor::new(self.config()?.build()?);
        let acceptor: s2n_tls_tokio::TlsAcceptor<Config> = acceptor;
        let acceptor = Arc::new(acceptor);

        let config = netbench::multiplex::Config::default();

        let mut conn_id = 0;
        loop {
            let (connection, _addr) = server.accept().await?;
            let scenario = scenario.clone();
            let id = conn_id;
            conn_id += 1;
            let acceptor = acceptor.clone();
            let trace = trace.clone();
            let config = config.clone();
            spawn(async move {
                if let Err(err) =
                    handle_connection(acceptor, connection, id, scenario, trace, config).await
                {
                    eprintln!("error: {}", err);
                }
            });
        }

        async fn handle_connection(
            acceptor: Arc<s2n_tls_tokio::TlsAcceptor<Config>>,
            connection: TcpStream,
            conn_id: u64,
            scenario: Arc<scenario::Server>,
            mut trace: impl netbench::Trace,
            config: multiplex::Config,
        ) -> Result<()> {
            let connection = acceptor.accept(connection).await?;
            let server_name = connection
                .get_ref()
                .server_name()
                .ok_or("missing server name")?;
            let scenario = scenario.on_server_name(server_name)?;

            let connection = Box::pin(connection);

            let conn = netbench::Driver::new(
                scenario,
                netbench::multiplex::Connection::new(conn_id, connection, config),
            );

            let mut checkpoints = HashSet::new();
            let mut timer = netbench::timer::Tokio::default();

            conn.run(&mut trace, &mut checkpoints, &mut timer).await?;

            Ok(())
        }
    }

    async fn server(&self) -> Result<TcpListener> {
        let server = TcpListener::bind((self.opts.ip, self.opts.port)).await?;
        Ok(server)
    }

    fn config(&self) -> Result<Builder, Error> {
        let (cert, private_key) = self.opts.certificate();

        let mut builder = Config::builder();
        builder.set_security_policy(&DEFAULT_TLS13)?;
        builder.load_pem(cert.pem.as_bytes(), private_key.pem.as_bytes())?;

        Ok(builder)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{multiplex, scenario, Result};
use netbench_driver::Allocator;
use s2n_tls::{
    config::{Builder, Config},
    error::Error,
    security::DEFAULT_TLS13,
};
use s2n_tls_tokio::{TlsConnector, TlsStream};
use std::{collections::HashSet, future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use structopt::StructOpt;
use tokio::net::TcpStream;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    Client::from_args().run().await
}

#[derive(Debug, StructOpt)]
pub struct Client {
    #[structopt(flatten)]
    opts: netbench_driver::Client,
}

impl Client {
    pub async fn run(&self) -> Result<()> {
        let addresses = self.opts.address_map().await?;
        let scenario = self.opts.scenario();

        let client = self.client()?;
        let client = netbench::Client::new(client, &scenario, &addresses);
        let mut trace = self.opts.trace();
        let mut checkpoints = HashSet::new();
        let mut timer = netbench::timer::Tokio::default();
        client.run(&mut trace, &mut checkpoints, &mut timer).await?;

        Ok(())
    }

    fn client(&self) -> Result<ClientImpl> {
        let connector = TlsConnector::new(self.config()?.build()?);
        let connector: s2n_tls_tokio::TlsConnector<Config> = connector;
        let connector = Arc::new(connector);

        let config = multiplex::Config::default();

        Ok(ClientImpl {
            config,
            connector,
            id: 0,
            nagle: self.opts.nagle,
        })
    }

    fn config(&self) -> Result<Builder, Error> {
        let mut builder = Config::builder();
        builder.set_security_policy(&DEFAULT_TLS13)?;
        for ca in self.opts.certificate_authorities() {
            builder.trust_pem(ca.pem.as_bytes())?;
        }
        Ok(builder)
    }
}

type Connection<'a> = netbench::Driver<'a, multiplex::Connection<TlsStream<TcpStream>>>;

#[derive(Clone)]
struct ClientImpl {
    config: multiplex::Config,
    connector: Arc<s2n_tls_tokio::TlsConnector<Config>>,
    id: u64,
    nagle: bool,
}

impl ClientImpl {
    fn id(&mut self) -> u64 {
        let id = self.id;
        self.id = id + 1;
        id
    }
}

impl<'a> netbench::client::Client<'a> for ClientImpl {
    type Connect = Pin<Box<dyn Future<Output = Result<Self::Connection>> + 'a>>;
    type Connection = Connection<'a>;

    fn connect(
        &mut self,
        addr: SocketAddr,
        server_name: &str,
        _server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let id = self.id();
        let config = self.config.clone();
        let connector = self.connector.clone();
        let nagle = self.nagle;
        let server_name = server_name.to_string();

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;

            if !nagle {
                let _ = conn.set_nodelay(true);
            }

            let conn = connector.connect(&server_name, conn).await?;
            let conn = Box::pin(conn);
            let conn = multiplex::Connection::new(id, conn, config);
            let conn: Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

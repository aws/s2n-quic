// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{multiplex, scenario, Result};
use netbench_driver::Allocator;
use std::{collections::HashSet, future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use structopt::StructOpt;
use tokio::{io::AsyncWriteExt, net::TcpStream};
use s2n_tls_tokio::{TlsConnector, TlsStream};
use s2n_tls::raw::{
    config::{Builder, Config},
    error::Error,
    security::DEFAULT_TLS13,
};

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
        let connector: s2n_tls_tokio::TlsConnector = connector.into();
        let connector = Arc::new(connector);

        let config = multiplex::Config::default();

        Ok(ClientImpl {
            config,
            connector,
            id: 0
        })
    }

    fn config(&self) -> Result<Builder, Error> {
        let mut builder = Config::builder();
        builder.set_security_policy(&DEFAULT_TLS13)?;
        for ca in self.opts.certificate_authorities() {
            builder.trust_pem(ca.pem.as_bytes())?;
        }
        unsafe {
            builder.disable_x509_verification()?;
        }
        Ok(builder)
    }
}

type Connection<'a> = netbench::Driver<'a, multiplex::Connection<TlsStream<TcpStream>>>;

#[derive(Clone)]
struct ClientImpl {
    config: multiplex::Config,
    connector: Arc<s2n_tls_tokio::TlsConnector>,
    id: u64,
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
        server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let id = self.id();
        let config = self.config.clone();
        let connector = self.connector.clone();
        let server_name = server_name.to_string();

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;
            let mut conn = connector.connect(&server_name, conn).await?;

            conn.write_u64(server_conn_id).await?;

            let conn = Box::pin(conn);
            let conn = multiplex::Connection::new(id, conn, config);
            let conn: Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

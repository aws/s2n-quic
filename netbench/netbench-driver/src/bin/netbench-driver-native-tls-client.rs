// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{multiplex, scenario, Result};
use std::{collections::HashSet, future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use structopt::StructOpt;
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_native_tls::{
    native_tls::{Certificate, TlsConnector},
    TlsStream,
};

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
        let mut builder = TlsConnector::builder();
        for ca in self.opts.certificate_authorities() {
            let ca = Certificate::from_pem(ca.pem.as_bytes())?;
            builder.add_root_certificate(ca);
        }
        let connector = builder.build()?;
        let connector: tokio_native_tls::TlsConnector = connector.into();
        let connector = Arc::new(connector);

        let config = multiplex::Config::default();

        Ok(ClientImpl { config, connector })
    }
}

type Connection<'a> = netbench::Driver<'a, multiplex::Connection<TlsStream<TcpStream>>>;

#[derive(Clone, Debug)]
struct ClientImpl {
    config: multiplex::Config,
    connector: Arc<tokio_native_tls::TlsConnector>,
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
        let config = self.config.clone();
        let connector = self.connector.clone();
        let server_name = server_name.to_string();

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;
            let mut conn = connector.connect(&server_name, conn).await?;

            // The native-tls crate does not expose the server name on the server so we need to
            // write the connection id for now.
            conn.write_u64(server_conn_id).await?;

            let conn = Box::pin(conn);
            let conn = multiplex::Connection::new(conn, config);
            let conn: Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

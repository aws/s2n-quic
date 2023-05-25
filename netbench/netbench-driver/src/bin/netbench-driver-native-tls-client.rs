// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{duplex, multiplex, scenario, Result};
use netbench_driver::Allocator;
use std::{collections::HashSet, future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use structopt::StructOpt;
use tokio::{
    io::{self, AsyncWriteExt},
    net::TcpStream,
};
use tokio_native_tls::{
    native_tls::{Certificate, TlsConnector},
    TlsStream,
};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

fn main() -> Result<()> {
    let args = Client::from_args();
    let runtime = args.opts.runtime();
    runtime.block_on(args.run())
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

        let mut trace = self.opts.trace();
        let mut checkpoints = HashSet::new();
        let mut timer = netbench::timer::Tokio::default();

        if let Some(config) = self.opts.multiplex() {
            let client = self.multiplex_client(config)?;
            let client = netbench::Client::new(client, &scenario, &addresses);
            client.run(&mut trace, &mut checkpoints, &mut timer).await?;
        } else {
            let client = self.duplex_client()?;
            let client = netbench::Client::new(client, &scenario, &addresses);
            client.run(&mut trace, &mut checkpoints, &mut timer).await?;
        }

        Ok(())
    }

    fn duplex_client(&self) -> Result<ClientImpl> {
        let mut builder = TlsConnector::builder();
        for ca in self.opts.certificate_authorities() {
            let ca = Certificate::from_pem(ca.pem.as_bytes())?;
            builder.add_root_certificate(ca);
        }
        let connector = builder.build()?;
        let connector: tokio_native_tls::TlsConnector = connector.into();
        let connector = Arc::new(connector);

        Ok(ClientImpl {
            connector,
            id: 0,
            rx_buffer: *self.opts.rx_buffer as _,
            tx_buffer: *self.opts.tx_buffer as _,
            nagle: self.opts.nagle,
        })
    }

    fn multiplex_client(&self, config: multiplex::Config) -> Result<MultiplexClientImpl> {
        let client = self.duplex_client()?;
        Ok(MultiplexClientImpl { client, config })
    }
}

type Stream = io::BufStream<TcpStream>;
type Connection<'a> = netbench::Driver<'a, duplex::Connection<TlsStream<Stream>>>;
type MultiplexConnection<'a> = netbench::Driver<'a, multiplex::Connection<TlsStream<Stream>>>;

#[derive(Clone, Debug)]
struct ClientImpl {
    connector: Arc<tokio_native_tls::TlsConnector>,
    id: u64,
    rx_buffer: usize,
    tx_buffer: usize,
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
        server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let id = self.id();
        let connector = self.connector.clone();
        let server_name = server_name.to_string();
        let rx_buffer = self.rx_buffer;
        let tx_buffer = self.tx_buffer;
        let nagle = self.nagle;

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;

            if !nagle {
                let _ = conn.set_nodelay(true);
            }

            let conn = io::BufStream::with_capacity(rx_buffer, tx_buffer, conn);

            let mut conn = connector.connect(&server_name, conn).await?;

            // The native-tls crate does not expose the server name on the server so we need to
            // write the connection id for now.
            conn.write_u64(server_conn_id).await?;

            let conn = Box::pin(conn);
            let conn = duplex::Connection::new(id, conn);
            let conn: Self::Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

#[derive(Clone, Debug)]
struct MultiplexClientImpl {
    config: multiplex::Config,
    client: ClientImpl,
}

impl<'a> netbench::client::Client<'a> for MultiplexClientImpl {
    type Connect = Pin<Box<dyn Future<Output = Result<Self::Connection>> + 'a>>;
    type Connection = MultiplexConnection<'a>;

    fn connect(
        &mut self,
        addr: SocketAddr,
        server_name: &str,
        server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let id = self.client.id();
        let config = self.config.clone();
        let connector = self.client.connector.clone();
        let server_name = server_name.to_string();
        let rx_buffer = self.client.rx_buffer;
        let tx_buffer = self.client.tx_buffer;
        let nagle = self.client.nagle;

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;

            if !nagle {
                let _ = conn.set_nodelay(true);
            }

            let conn = io::BufStream::with_capacity(rx_buffer, tx_buffer, conn);

            let mut conn = connector.connect(&server_name, conn).await?;

            // The native-tls crate does not expose the server name on the server so we need to
            // write the connection id for now.
            conn.write_u64(server_conn_id).await?;

            let conn = Box::pin(conn);
            let conn = multiplex::Connection::new(id, conn, config);
            let conn: Self::Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

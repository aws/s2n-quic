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
        Ok(ClientImpl {
            id: 0,
            nagle: self.opts.nagle,
            rx_buffer: *self.opts.rx_buffer as _,
            tx_buffer: *self.opts.tx_buffer as _,
        })
    }

    fn multiplex_client(&self, config: multiplex::Config) -> Result<MultiplexClientImpl> {
        let client = self.duplex_client()?;
        Ok(MultiplexClientImpl { config, client })
    }
}

type Stream = io::BufStream<TcpStream>;
type Connection<'a> = netbench::Driver<'a, duplex::Connection<Stream>>;
type MultiplexConnection<'a> = netbench::Driver<'a, multiplex::Connection<Stream>>;

#[derive(Debug)]
struct ClientImpl {
    id: u64,
    nagle: bool,
    rx_buffer: usize,
    tx_buffer: usize,
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
        _server_name: &str,
        server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let id = self.id();
        let rx_buffer = self.rx_buffer;
        let tx_buffer = self.tx_buffer;
        let nagle = self.nagle;

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;

            if !nagle {
                let _ = conn.set_nodelay(true);
            }

            let conn = io::BufStream::with_capacity(rx_buffer, tx_buffer, conn);
            let mut conn = Box::pin(conn);

            // tell the server which connection ID to use
            conn.write_u64(server_conn_id).await?;

            let conn = duplex::Connection::new(id, conn);
            let conn: Self::Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}
#[derive(Debug)]
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
        _server_name: &str,
        server_conn_id: u64,
        scenario: &'a Arc<scenario::Connection>,
    ) -> Self::Connect {
        let id = self.client.id();
        let config = self.config.clone();
        let rx_buffer = self.client.rx_buffer;
        let tx_buffer = self.client.tx_buffer;
        let nagle = self.client.nagle;

        let fut = async move {
            let conn = TcpStream::connect(addr).await?;

            if !nagle {
                let _ = conn.set_nodelay(true);
            }

            let conn = io::BufStream::with_capacity(rx_buffer, tx_buffer, conn);
            let mut conn = Box::pin(conn);

            // tell the server which connection ID to use
            conn.write_u64(server_conn_id).await?;

            let conn = multiplex::Connection::new(id, conn, config);
            let conn: Self::Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

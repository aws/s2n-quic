// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{multiplex, scenario, Result};
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

        // TODO pull from scenario configuration
        let config = multiplex::Config::default();

        let client = ClientImpl {
            config,
            id: 0,
            nagle: self.opts.nagle,
            rx_buffer: *self.opts.rx_buffer as _,
            tx_buffer: *self.opts.tx_buffer as _,
        };

        let client = netbench::Client::new(client, &scenario, &addresses);
        let mut trace = self.opts.trace();
        let mut checkpoints = HashSet::new();
        let mut timer = netbench::timer::Tokio::default();
        client.run(&mut trace, &mut checkpoints, &mut timer).await?;

        Ok(())
    }
}

type Stream = io::BufStream<TcpStream>;
type Connection<'a> = netbench::Driver<'a, multiplex::Connection<Stream>>;

#[derive(Debug)]
struct ClientImpl {
    config: multiplex::Config,
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
        let config = self.config.clone();
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

            let conn = multiplex::Connection::new(id, conn, config);
            let conn: Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

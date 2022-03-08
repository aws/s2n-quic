// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{multiplex, scenario, Result};
use std::{collections::HashSet, future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use structopt::StructOpt;
use tokio::{io::AsyncWriteExt, net::TcpStream};

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

        let client = ClientImpl { config };

        let client = netbench::Client::new(client, &scenario, &addresses);
        let mut trace = self.opts.trace();
        let mut checkpoints = HashSet::new();
        let mut timer = netbench::timer::Tokio::default();
        client.run(&mut trace, &mut checkpoints, &mut timer).await?;

        Ok(())
    }
}

type Connection<'a> = netbench::Driver<'a, multiplex::Connection<TcpStream>>;

#[derive(Clone, Debug)]
struct ClientImpl {
    config: multiplex::Config,
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
        let config = self.config.clone();

        let fut = async move {
            let mut conn = TcpStream::connect(addr).await?;

            // tell the server which connection ID to use
            conn.write_u64(server_conn_id).await?;

            let conn = Box::pin(conn);
            let conn = multiplex::Connection::new(conn, config);
            let conn: Connection = netbench::Driver::new(scenario, conn);

            Result::Ok(conn)
        };

        Box::pin(fut)
    }
}

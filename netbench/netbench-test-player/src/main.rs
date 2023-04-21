// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod endpoint;
mod status;

use endpoint::{client_state_machine, server_state_machine, EndpointKind};
use netbench::{collector, Result};
use structopt::StructOpt;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tokio;

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(flatten)]
    collector_args: collector::Args,

    /// The address from which we receive status messages about the
    /// peer under test.
    #[structopt(long)]
    pub remote_status_server: SocketAddr,

    /// The port from which we report our status.
    #[structopt(long, default_value = "8080")]
    pub local_status_port: u16,

    /// Do we run as a "server" or a "client".
    #[structopt(long)]
    pub run_as: EndpointKind,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let args = Args::from_args();

    match args.run_as {
        EndpointKind::Server => server_state_machine(args.collector_args, SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            args.local_status_port,
        )).await?,
        EndpointKind::Client => client_state_machine(args.collector_args, args.remote_status_server).await?,
    };
    Ok(())
}

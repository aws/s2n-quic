// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod endpoint;
mod status;

use endpoint::{client_state_machine, server_state_machine, EndpointKind};
use futures::FutureExt;
use netbench::{collector, Result};
use status::StatusTracker;
use structopt::StructOpt;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use futures::select;
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
async fn main() -> Result<(), &'static str> {
    let args = Args::from_args();

    let state_tracker = StatusTracker::new(
        args.remote_status_server,
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            args.local_status_port,
        ),
    );
    let state_tracker_machine = state_tracker.clone();

    match args.run_as {
        EndpointKind::Server => select!(
            _ = Box::pin(state_tracker.state_server().fuse()) => Err("The status server had and internal error."),
            _ = Box::pin(server_state_machine(args.collector_args, state_tracker_machine).fuse()) => Ok(()),
        ),
        EndpointKind::Client => select!(
            _ = Box::pin(state_tracker.state_server().fuse()) => Err("The status server had an internal error."),
            _ = Box::pin(client_state_machine(args.collector_args, state_tracker_machine).fuse()) => Ok(()),
        ),
        _ => unimplemented!("Only --run-as client and --run-as server are supported."),
    }
}

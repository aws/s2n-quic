// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod endpoint;
mod status;

use endpoint::{client_state_machine, server_state_machine, EndpointKind};
use netbench::{collector, Result};
use status::StatusTracker;
use structopt::StructOpt;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tokio::try_join;

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

    /// Are we a server, client or router?
    #[structopt(long)]
    pub run_as: EndpointKind,

    // Should we output to the log?
    #[structopt(long, short)]
    pub verbose: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::from_args();

    let state_tracker = StatusTracker::new(
        args.remote_status_server,
        SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            args.local_status_port,
        ),
        args.verbose,
    );
    let state_tracker_clone = state_tracker.clone();
    match args.run_as {
        EndpointKind::Server => {
            try_join!(
                state_tracker.state_server(),
                server_state_machine(args.collector_args, state_tracker_clone)
            )?;
        }
        EndpointKind::Client => {
            try_join!(
                state_tracker.state_server(),
                client_state_machine(args.collector_args, state_tracker_clone)
            )?;
        }
        _ => unimplemented!("Only --run-as client and --run-as server are supported."),
    };
    Ok(())
}

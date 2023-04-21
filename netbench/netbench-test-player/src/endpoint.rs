// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::status::{Status, StatusTracker};
use netbench::collector::{run, Args, RunHandle};

use std::time::Duration;
use strum_macros::{Display, EnumString};
use tokio::io::Result;
use tokio::net::ToSocketAddrs;
use tokio::time::sleep;

/// For the purposes of coordination, what is the kind of endpoint we are?
#[derive(Debug, PartialEq, Clone, Copy, EnumString, Hash, Display)]
#[strum(serialize_all = "lowercase")]
pub enum EndpointKind {
    /// Servers should start after routers are online. They should stop after
    /// clients report they are finished.
    Server,
    /// Clients should start after servers are online. They will stop when
    /// finished running their scenario.
    Client,
}

/// The main implementation for --run-as server.
///
/// This steps through the states of the server, waiting on the client when
/// necessary.
pub async fn server_state_machine<A: ToSocketAddrs>(
    args: Args,
    local_status_server: A,
) -> Result<()> {
    // Wait for client to be up; and establish connection.
    let mut status_tracker = StatusTracker::new_as_server(local_status_server).await;

    // Run the collector in the background.
    let collector = run(args).await;

    // Signal Client we are running after waiting to collect server startup data.
    sleep(Duration::from_secs(10)).await;
    status_tracker.signal_status(Status::Running).await?;

    // Run until the client reports it is finished.
    status_tracker.wait_for_peer(Status::Finished).await?;

    // Kill the collector after waiting to collect server spin down data.
    sleep(Duration::from_secs(10)).await;
    collector.kill().expect("Failed to kill child?");

    Ok(())
}

/// The main implementation for --run-as client.
///
/// This steps through the states of the client, waiting on the server when
/// necessary.
pub async fn client_state_machine<A: ToSocketAddrs + Copy>(
    args: Args,
    state_server_address: A,
) -> Result<()> {
    // Wait for the server to be up and establish connection.
    let mut status_tracker = StatusTracker::new_as_client(state_server_address).await;

    // Wait for the server to signal it is running.
    status_tracker.wait_for_peer(Status::Running).await?;

    // Run the client till finished.
    run(args).await.wait().expect("Waiting on child failed?");

    // Signal Server to finish.
    status_tracker.signal_status(Status::Finished).await?;
    Ok(())
}

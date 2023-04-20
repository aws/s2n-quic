// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::status::{Status, StatusTracker};
use netbench::collector::{run, Args, RunHandle};

use strum_macros::EnumString;

/// For the purposes of coordination, what is the kind of endpoint we are?
#[derive(Debug, PartialEq, Clone, Copy, EnumString, Hash)]
#[strum(ascii_case_insensitive)]
pub enum EndpointKind {
    /// A Router comes up first and goes down last. Routers should not stop
    /// running unless explicitly instructed to by the test framework.
    Router,
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
///
/// Return an error when finished to end a try_join!() this task may be
/// a part of.
pub async fn server_state_machine(args: Args, mut state_tracker: StatusTracker) -> Result<(), ()> {
    state_tracker.store(Status::Ready);

    // Wait till our peer reports it is Ready
    state_tracker.wait_for_peer_ready().await;

    // Run the collector in the background
    let child_handle = run(args).await;
    state_tracker.store(Status::Running);

    // Run until the client reports it is Finished
    state_tracker.wait_for_peer_finished().await;

    child_handle.kill().expect("Failed to kill child?");
    // We are done
    state_tracker.store(Status::Finished);

    Err(())
}

/// The main implementation for --run-as client.
///
/// This steps through the states of the client, waiting on the server when
/// necessary.
///
/// Return an error when finished to end a try_join!() this task may be
/// a part of.
pub async fn client_state_machine(args: Args, mut state_tracker: StatusTracker) -> Result<(), ()> {
    state_tracker.store(Status::Ready);

    // Wait for the server to be running
    state_tracker.wait_for_peer_running().await;

    // Run until finished
    let handle = run(args).await;
    state_tracker.store(Status::Running);
    handle.wait().unwrap();

    // Finished
    state_tracker.store(Status::Finished);
    Err(())
}

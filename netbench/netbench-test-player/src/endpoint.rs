use crate::status::{Status, StatusTracker};
use netbench::collector::{run, Args, RunHandle};

use std::{io::ErrorKind, time::Duration};

use strum_macros::EnumString;

use tokio::{
    io::{self},
    task::JoinHandle,
    try_join,
};

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
pub fn server_state_machine(
    args: Args,
    mut state_tracker: StatusTracker,
) -> JoinHandle<io::Result<()>> {
    tokio::spawn(async move {
        state_tracker.store(Status::Ready);

        // Wait till our peer reports it is Ready
        try_join!(state_tracker.wait_for_peer(
            Status::Ready,
            Status::NotReady,
            Duration::from_secs(5),
            Duration::from_secs(5)
        ))?;

        state_tracker.store(Status::Running);
        // Run until the server reports it is Finished
        let (_finished_waiting, child) =
            try_join!(state_tracker.wait_for_peer_finished(), run(args))?;
        child.kill().expect("Failed to kill child?");

        // We are done
        state_tracker.store(Status::Finished);

        Err(io::Error::new(ErrorKind::Other, String::from("Finished")))
    })
}

/// The main implementation for --run-as client.
///
/// This steps through the states of the client, waiting on the server when
/// necessary.
///
/// Return an error when finished to end a try_join!() this task may be
/// a part of.
pub fn client_state_machine(
    args: Args,
    mut state_tracker: StatusTracker,
) -> JoinHandle<io::Result<()>> {
    tokio::spawn(async move {
        state_tracker.store(Status::Ready);

        // Wait for the server to be running
        try_join!(state_tracker.wait_for_peer_ready())?;

        // Run until finished
        state_tracker.store(Status::Running);
        let (handle,) = try_join!(run(args))?;
        handle.wait().unwrap();

        // Finished
        state_tracker.store(Status::Finished);
        Err(io::Error::new(ErrorKind::Other, "Finished"))
    })
}

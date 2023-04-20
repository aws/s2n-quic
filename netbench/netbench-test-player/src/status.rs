// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use num_enum::{IntoPrimitive, TryFromPrimitive};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

use tracing::{info, instrument};

/// The status of an endpoint; used to coordinate actions.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, IntoPrimitive, TryFromPrimitive,
)]
#[repr(u8)]
pub enum Status {
    /// Still preparing the environment for the test.
    ///
    /// This may include installing dependencies, building drivers,
    /// ect. endpoints may spend a minutes in this state.
    NotReady,
    /// Ready to begin running the running the collector and driver,
    /// but have not started yet.
    Ready,
    /// Running the collector and driver.
    Running,
    /// Finished, the collector and driver are no longer running.
    Finished,
}

#[derive(Debug, Clone)]
pub struct StatusTracker {
    // this endpoint's current state as an atomicU8 for safe sharing between threads
    pub current_state: Arc<AtomicU8>,
    pub remote_status_server: SocketAddr,
    pub local_status_server: SocketAddr,
}

impl StatusTracker {
    pub fn new(remote_status_server: SocketAddr, local_status_server: SocketAddr) -> Self {
        Self {
            current_state: Arc::new(AtomicU8::new(Status::NotReady.into())),
            local_status_server,
            remote_status_server,
        }
    }

    /// Update our own state
    #[instrument]
    pub fn store(&mut self, value: Status) {
        self.current_state.store(value.into(), Ordering::Relaxed)
    }

    /// Query peer for state; If connection fails return `assume_on_no_response`
    #[instrument]
    async fn get_status(remote_status_server: SocketAddr, assume_on_no_response: Status) -> Status {
        loop {
            let mut stream = match TcpStream::connect(remote_status_server).await {
                Ok(stream) => stream,
                Err(_) => return assume_on_no_response,
            };
            let mut buffer = Vec::new();
            match stream
                .read_to_end(&mut buffer)
                .await {
                Ok(_) => return serde_json::from_slice(&buffer).expect("Failed to parse peer's status"),
                Err(_) => continue,
            }
        }
    }

    /// A Task that waits for peer to be in a particular state
    #[instrument]
    pub async fn wait_for_peer(
        &self,
        wait_for_status: Status,
        assume_on_no_response: Status,
        initial_delay: Duration,
        poll_delay: Duration,
    ) {
        let remote_status_server = self.remote_status_server;
        sleep(initial_delay).await;
        loop {
            match Self::get_status(remote_status_server, assume_on_no_response).await {
                s if s == wait_for_status => break,
                peer_reported_status => {
                    info!(?peer_reported_status);
                    sleep(poll_delay).await
                }
            }
            info!(?wait_for_status)
        }
    }

    /// A Task that waits for the peer to report a finished status
    pub async fn wait_for_peer_finished(&self) {
        self.wait_for_peer(
            Status::Finished,
            // If we don't hear from the peer, assume it is finished
            Status::Finished,
            // Don't request status updates for the first 10 seconds
            Duration::from_secs(10),
            // Then request one every 5 seconds till the end of the test
            Duration::from_secs(5),
        )
        .await;
    }

    /// A task that wait until the peer reports it is ready
    pub async fn wait_for_peer_running(&self) {
        self.wait_for_peer(
            Status::Running,
            // Assume peer isn't ready, unless we hear from it
            Status::NotReady,
            // Just ask again every 5 seconds
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
        .await;
    }

    /// A task that wait until the peer reports it is ready
    pub async fn wait_for_peer_ready(&self) {
        self.wait_for_peer(
            Status::Ready,
            // Assume peer isn't ready, unless we hear from it
            Status::NotReady,
            // Just ask again every 5 seconds
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
        .await;
    }

    /// A Task that serves our state, when the peer asks for it
    #[instrument]
    pub async fn state_server(&self) -> Result<(), ()> {
        let listener = TcpListener::bind(self.local_status_server)
            .await
            .expect("Error binding to socket.");

        let current_state = self.current_state.clone();

        let mut served_state = Status::NotReady;
        loop {
            if served_state == Status::Finished {
                break Err(());
            }

            let (mut socket, _) = match timeout(Duration::from_secs(30), listener.accept()).await {
                Ok(Ok(o)) => o,
                _ => continue,
            };

            served_state = current_state
                .clone()
                .load(Ordering::Relaxed)
                .try_into()
                .expect("An invalid atomic u8 got constructed.");

            socket
                .write_all(
                    &serde_json::to_vec(&served_state).expect("State couldn't be serialized?"),
                )
                .await
                .expect("Error writing to socket.");

            info!(?served_state);
        }
    }
}

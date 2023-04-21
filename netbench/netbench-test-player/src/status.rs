// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, Result},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    time::{sleep, timeout},
};
use std::fmt::Debug;

/// TIMEOUT after an hour. If the SSM commands haven't made it to all
/// the machines in that time; abort the test.
const TIMEOUT: Duration = Duration::from_secs(60*60);

use tracing::instrument;

/// The status of an endpoint; used to coordinate actions.
#[derive(Debug, Clone, Copy, PartialEq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Status {
    /// Running the collector and driver.
    Running,
    /// Finished, the collector and driver are no longer running.
    Finished,
}

#[derive(Debug)]
pub struct StatusTracker {
    /// A connection to the peer.
    pub peer_connection: TcpStream,
}

impl StatusTracker {
    /// Establish a connection to the server, if the server isn't up keep trying till it is.
    /// Once a StatusTracker exists we know implicitly that the server is online.
    pub async fn new_as_client<A: ToSocketAddrs + Copy>(state_server_address: A) -> StatusTracker {
        timeout(TIMEOUT, async {
            loop {
                match TcpStream::connect(state_server_address).await {
                    Ok(peer_connection) => return StatusTracker { peer_connection },
                    Err(_) => sleep(Duration::from_secs(10)).await,
                }
            }
        }).await.unwrap()
    }

    /// Allow a single client to connect. Once a StatusTracker exists the client
    /// is connected.
    pub async fn new_as_server<A: ToSocketAddrs>(local_status_server: A) -> StatusTracker {
        let (peer_connection, _addr) = timeout(TIMEOUT, TcpListener::bind(local_status_server).await.unwrap().accept()).await.unwrap().unwrap();
        StatusTracker { peer_connection }
    }

    /// Wait until the peer signals it's status is `wait_for`. This doesn't
    /// poll with the client; it simply awaits and processes messages until
    /// `wait_for` is received.
    #[instrument]
    pub async fn wait_for_peer(&mut self, wait_for: Status) -> Result<()> {
        loop {
            let status: Status = self.peer_connection.read_u8().await?.try_into().expect("Bad status received");
            if wait_for == status {
                return Ok(());
            }
        }
    }

    /// Signal our status to the peer.
    #[instrument]
    pub async fn signal_status(&mut self, status: Status) -> Result<()> {
        self.peer_connection.write_u8(status.into()).await
    }
}
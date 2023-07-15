// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use core::time::Duration;
use rand::prelude::*;
use std::net::SocketAddr;
use tokio::{net::UdpSocket, task::JoinSet};

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T = (), E = Error> = core::result::Result<T, E>;

fn main() -> Result {
    Args::parse().run()
}

#[derive(Debug, Parser)]
struct Args {
    /// The local address to bind the workers to
    #[arg(long, default_value = "0.0.0.0:0")]
    local_address: String,

    /// The number of workers to run concurrently
    #[arg(long, default_value_t = 100)]
    workers: u16,

    /// The maximum packet size to generate
    #[arg(long, default_value_t = 1500)]
    mtu: u16,

    /// The target of the UDP endpoint
    #[arg(default_value = "localhost:443")]
    address: String,
}

impl Args {
    #[tokio::main]
    async fn run(self) -> Result {
        let remote_address = tokio::net::lookup_host(&self.address)
            .await?
            .next()
            .unwrap();

        let local_address: SocketAddr = self.local_address.parse()?;

        let mut set = JoinSet::new();

        for _ in 0..self.workers {
            set.spawn(worker(remote_address, local_address, self.mtu as _));
        }

        while set.join_next().await.is_some() {}

        Ok(())
    }
}

async fn sleep_rand() {
    let ms = thread_rng().gen_range(0..50);
    if ms > 0 {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

async fn worker(remote_address: SocketAddr, local_address: SocketAddr, mtu: usize) -> Result<()> {
    let socket = UdpSocket::bind(local_address).await?;

    let mut payload = vec![];

    loop {
        let burst = thread_rng().gen_range(1..100);
        for _ in 0..burst {
            generate_payload(&mut payload, mtu);
            let _ = socket.send_to(&payload, remote_address).await;
        }
        sleep_rand().await;
    }
}

fn generate_payload(payload: &mut Vec<u8>, mtu: usize) {
    let len = thread_rng().gen_range(0..=mtu);
    payload.resize(len, 0);
    thread_rng().fill_bytes(payload);
}

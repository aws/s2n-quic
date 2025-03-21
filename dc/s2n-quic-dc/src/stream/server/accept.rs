// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        application::{Builder as StreamBuilder, Stream},
        server::stats,
    },
    sync::mpmc as channel,
};
use std::{io, net::SocketAddr};

mod pruner;

pub use pruner::Pruner;

#[derive(Clone, Copy, Debug, Default)]
pub enum Flavor {
    #[default]
    Fifo,
    Lifo,
}

pub type Sender<Sub> = channel::Sender<StreamBuilder<Sub>>;
pub type Receiver<Sub> = channel::Receiver<StreamBuilder<Sub>>;

#[inline]
pub fn channel<Sub>(capacity: usize) -> (Sender<Sub>, Receiver<Sub>)
where
    Sub: event::Subscriber,
{
    channel::new(capacity)
}

#[inline]
pub async fn accept<Sub>(
    streams: &Receiver<Sub>,
    stats: &stats::Sender,
) -> io::Result<(Stream<Sub>, SocketAddr)>
where
    Sub: event::Subscriber,
{
    let stream = streams.recv_front().await.map_err(|_err| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "server acceptor runtime is no longer available",
        )
    })?;

    // build the stream inside the application context
    let (stream, sojourn_time) = stream.accept()?;
    stats.send(sojourn_time);

    let remote_addr = stream.peer_addr()?;

    Ok((stream, remote_addr))
}

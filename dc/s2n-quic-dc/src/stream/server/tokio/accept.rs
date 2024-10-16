// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::stats;
use crate::{
    stream::{
        application::{Builder as StreamBuilder, Stream},
        environment::{tokio::Environment, Environment as _},
    },
    sync::channel,
};
use core::time::Duration;
use s2n_quic_core::time::{Clock, Timestamp};
use std::{io, net::SocketAddr};
use tokio::time::sleep;

#[derive(Clone, Copy, Default)]
pub enum Flavor {
    #[default]
    Fifo,
    Lifo,
}

pub type Sender = channel::Sender<(StreamBuilder, Timestamp)>;
pub type Receiver = channel::Receiver<(StreamBuilder, Timestamp)>;

#[inline]
pub async fn accept(streams: &Receiver, stats: &stats::Sender) -> io::Result<(Stream, SocketAddr)> {
    let (stream, queue_time) = streams.recv_front().await.map_err(|_err| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "server acceptor runtime is no longer available",
        )
    })?;

    let now = stream.shared.common.clock.get_time();
    let sojourn_time = now.saturating_duration_since(queue_time);

    // submit sojourn time statistics
    stats.send(sojourn_time);

    // build the stream inside the application context
    let stream = stream.build()?;
    let remote_addr = stream.peer_addr()?;

    Ok((stream, remote_addr))
}

#[derive(Clone, Debug)]
pub struct Pruner {
    /// Any sojourn duration multiplied by this value is unlikely to be accepted in time
    pub sojourn_multiplier: u32,

    /// Don't prune anything under this amount, just so we can handle bursts of streams and
    /// not prematurely drop things.
    pub min_threshold: Duration,

    /// Anything older than this amount has likely timed out at this point. No need to hold
    /// on to the stream any longer at this point
    pub max_threshold: Duration,

    /// Minimum amount of time to sleep before pruning the queue again
    pub min_sleep_time: Duration,

    /// Maximum amount of time to sleep before pruning the queue again
    pub max_sleep_time: Duration,
}

impl Default for Pruner {
    fn default() -> Self {
        Self {
            sojourn_multiplier: 3,
            min_threshold: Duration::from_millis(100),
            max_threshold: Duration::from_secs(5),
            min_sleep_time: Duration::from_millis(100),
            max_sleep_time: Duration::from_secs(1),
        }
    }
}

impl Pruner {
    /// A task which prunes the accept queue to enforce a maximum sojourn time
    pub async fn run(
        self,
        env: Environment,
        channel: channel::WeakReceiver<(StreamBuilder, Timestamp)>,
        stats: stats::Stats,
    ) {
        let Self {
            sojourn_multiplier,
            min_threshold,
            max_threshold,
            min_sleep_time,
            max_sleep_time,
        } = self;

        sleep(min_sleep_time).await;

        loop {
            let now = env.clock().get_time();
            let smoothed_sojourn_time = stats.smoothed_sojourn_time();

            // compute the oldest allowed queue time
            let Some(queue_time_threshold) = now.checked_sub(
                (smoothed_sojourn_time * sojourn_multiplier).clamp(min_threshold, max_threshold),
            ) else {
                sleep(min_sleep_time).await;
                continue;
            };

            // Use optional locks to avoid lock contention. If there is contention on the channel, the
            // old streams will naturally be pruned, since old ones will be dropped in favor of new
            // ones.
            let priority = channel::Priority::Optional;

            loop {
                // pop off any items that have expired
                let res = channel.pop_back_if(priority, |(_stream, queue_time)| {
                    queue_time.has_elapsed(queue_time_threshold)
                });

                match res {
                    // we pruned a stream
                    Ok(Some((stream, queue_time))) => {
                        tracing::debug!(
                            event = "accept::prune",
                            credentials = ?stream.shared.credentials(),
                            queue_duration = ?now.saturating_duration_since(queue_time),
                        );
                        continue;
                    }
                    // no more streams left to prune
                    Ok(None) => break,
                    // the channel was closed
                    Err(_) => return,
                }
            }

            // wake up later based on the smoothed sojourn time
            sleep(smoothed_sojourn_time.clamp(min_sleep_time, max_sleep_time)).await;
        }
    }
}

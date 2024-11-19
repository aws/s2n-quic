// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::stats;
use crate::{
    event::{self, IntoEvent, Subscriber},
    stream::{
        application::{Builder as StreamBuilder, Stream},
        environment::{tokio::Environment, Environment as _},
    },
    sync::channel,
};
use core::time::Duration;
use s2n_quic_core::time::Clock;
use std::{io, net::SocketAddr};
use tokio::time::sleep;

#[derive(Clone, Copy, Default)]
pub enum Flavor {
    #[default]
    Fifo,
    Lifo,
}

pub type Sender = channel::Sender<StreamBuilder>;
pub type Receiver = channel::Receiver<StreamBuilder>;

#[inline]
pub fn channel(capacity: usize) -> (Sender, Receiver) {
    channel::new(capacity)
}

#[inline]
pub async fn accept<Sub>(
    streams: &Receiver,
    stats: &stats::Sender,
    subscriber: &Sub,
) -> io::Result<(Stream, SocketAddr)>
where
    Sub: Subscriber,
{
    let stream = streams.recv_front().await.map_err(|_err| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "server acceptor runtime is no longer available",
        )
    })?;

    let publisher = event::EndpointPublisherSubscriber::new(
        event::builder::EndpointMeta {
            timestamp: stream.shared.clock.get_time().into_event(),
        },
        None,
        subscriber,
    );

    // build the stream inside the application context
    let (stream, sojourn_time) = stream.build(&publisher)?;
    stats.send(sojourn_time);

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
    pub async fn run<Sub>(
        self,
        env: Environment,
        channel: channel::WeakReceiver<StreamBuilder>,
        stats: stats::Stats,
        subscriber: Sub,
    ) where
        Sub: Subscriber,
    {
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

            let publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    timestamp: now.into_event(),
                },
                None,
                &subscriber,
            );

            loop {
                // pop off any items that have expired
                let res = channel.pop_back_if(priority, |stream| {
                    stream.queue_time.has_elapsed(queue_time_threshold)
                });

                match res {
                    // we pruned a stream
                    Ok(Some(stream)) => {
                        stream.prune(
                            event::builder::AcceptorStreamPruneReason::MaxSojournTimeExceeded,
                            &publisher,
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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Credits;
use crate::stream::send::{error::Error, flow};
use atomic_waker::AtomicWaker;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};
use s2n_quic_core::{ensure, varint::VarInt};

const ERROR_MASK: u64 = 1 << 63;
const FINISHED_MASK: u64 = 1 << 62;

pub struct State {
    /// Monotonic offset which tracks where the application is currently writing
    stream_offset: AtomicU64,
    /// Monotonic offset which indicates the maximum offset the application can write to
    flow_offset: AtomicU64,
    /// Notifies an application of newly-available flow credits
    poll_waker: AtomicWaker,
    // TODO add a list for the `acquire` future wakers
}

impl State {
    #[inline]
    pub fn new(initial_flow_offset: VarInt) -> Self {
        Self {
            stream_offset: AtomicU64::new(0),
            flow_offset: AtomicU64::new(initial_flow_offset.as_u64()),
            poll_waker: AtomicWaker::new(),
        }
    }
}

impl State {
    /// Called by the background worker to release flow credits
    ///
    /// Callers MUST ensure the provided offset is monotonic.
    #[inline]
    pub fn release(&self, flow_offset: VarInt) {
        self.flow_offset
            .store(flow_offset.as_u64(), Ordering::Release);
        self.poll_waker.wake();
    }

    /// Called by the application to acquire flow credits
    #[inline]
    pub async fn acquire(&self, request: flow::Request) -> Result<Credits, Error> {
        core::future::poll_fn(|cx| self.poll_acquire(cx, request)).await
    }

    /// Called by the application to acquire flow credits
    #[inline]
    pub fn poll_acquire(
        &self,
        cx: &mut Context,
        mut request: flow::Request,
    ) -> Poll<Result<Credits, Error>> {
        let mut current_offset = self.acquire_offset()?;

        let mut stored_waker = false;

        loop {
            let flow_offset = self.flow_offset.load(Ordering::Acquire);

            let Some(flow_credits) = flow_offset
                .checked_sub(current_offset.as_u64())
                .filter(|v| {
                    // if we're finishing the stream and don't have any buffered data, then we
                    // don't need any flow control
                    if request.len == 0 && request.is_fin {
                        true
                    } else {
                        *v > 0
                    }
                })
            else {
                // if we already stored a waker and didn't get more credits then yield the task
                ensure!(!stored_waker, Poll::Pending);
                stored_waker = true;

                self.poll_waker.register(cx.waker());

                // make one last effort to acquire some flow credits before going to sleep
                current_offset = self.acquire_offset()?;

                continue;
            };

            // clamp the request to the flow credits we have
            request.clamp(flow_credits);

            let mut new_offset = current_offset
                .as_u64()
                .checked_add(request.len as u64)
                .ok_or(Error::PayloadTooLarge)?;

            // record that we've sent the final offset
            if request.is_fin {
                new_offset |= FINISHED_MASK;
            }

            let result = self.stream_offset.compare_exchange(
                current_offset.as_u64(),
                new_offset,
                Ordering::Release, // TODO is this the correct ordering?
                Ordering::Acquire,
            );

            match result {
                Ok(_) => {
                    // the offset was correctly updated so return our acquired credits
                    let credits = request.response(current_offset);
                    return Poll::Ready(Ok(credits));
                }
                Err(updated_offset) => {
                    // the offset was updated from underneath us so try again
                    current_offset = Self::process_offset(updated_offset)?;
                    // clear the fact that we stored the waker, since we need to do a full sync
                    // to get the correct state
                    stored_waker = false;
                    continue;
                }
            }
        }
    }

    #[inline]
    fn acquire_offset(&self) -> Result<VarInt, Error> {
        Self::process_offset(self.stream_offset.load(Ordering::Acquire))
    }

    #[inline]
    fn process_offset(offset: u64) -> Result<VarInt, Error> {
        if offset & ERROR_MASK == ERROR_MASK {
            // TODO actually load the error value for the stream
            return Err(Error::TransportError { code: VarInt::MAX });
        }

        if offset & FINISHED_MASK == FINISHED_MASK {
            return Err(Error::FinalSizeChanged);
        }

        Ok(unsafe { VarInt::new_unchecked(offset) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::send::path;
    use std::sync::Arc;

    #[tokio::test]
    async fn concurrent_flow() {
        let mut initial_offset = VarInt::from_u8(255);
        let expected_len = VarInt::from_u16(u16::MAX);
        let state = Arc::new(State::new(initial_offset));
        let path_info = path::Info {
            mtu: 1500,
            send_quantum: 10,
            ecn: Default::default(),
            next_expected_control_packet: Default::default(),
        };
        let total = Arc::new(AtomicU64::new(0));
        // TODO support more than one Waker via intrusive list or something
        let workers = 1;
        let worker_counts = Vec::from_iter((0..workers).map(|_| Arc::new(AtomicU64::new(0))));

        let mut tasks = tokio::task::JoinSet::new();

        for (idx, count) in worker_counts.iter().cloned().enumerate() {
            let total = total.clone();
            let state = state.clone();
            tasks.spawn(async move {
                tokio::time::sleep(core::time::Duration::from_millis(10)).await;

                let mut buffer_len = 1;
                let mut is_fin = false;
                let max_segments = 10;
                let max_header_len = 50;
                let mut max_offset = VarInt::ZERO;

                loop {
                    let mut request = flow::Request {
                        len: buffer_len,
                        is_fin,
                    };
                    request.clamp(path_info.max_flow_credits(max_header_len, max_segments));

                    let Ok(credits) = state.acquire(request).await else {
                        break;
                    };

                    println!(
                        "thread={idx} offset={}..{}",
                        credits.offset,
                        credits.offset + credits.len
                    );
                    buffer_len += 1;
                    buffer_len = buffer_len.min(
                        expected_len
                            .as_u64()
                            .saturating_sub(credits.offset.as_u64())
                            .saturating_sub(credits.len as u64) as usize,
                    );
                    assert!(max_offset <= credits.offset);
                    max_offset = credits.offset;
                    if buffer_len == 0 {
                        is_fin = true;
                    }
                    total.fetch_add(credits.len as _, Ordering::Relaxed);
                    count.fetch_add(credits.len as _, Ordering::Relaxed);
                }
            });
        }

        tasks.spawn(async move {
            let mut credits = 10;
            while initial_offset < expected_len {
                tokio::time::sleep(core::time::Duration::from_millis(1)).await;
                initial_offset = (initial_offset + credits).min(expected_len);
                credits += 1;
                state.release(initial_offset);
            }
        });

        // make sure all of the tasks complete
        while tasks.join_next().await.is_some() {}

        assert_eq!(total.load(Ordering::Relaxed), expected_len.as_u64());
        let mut at_least_one_write = true;
        for (idx, count) in worker_counts.into_iter().enumerate() {
            let count = count.load(Ordering::Relaxed);
            println!("thread={idx}, count={}", count);
            if count == 0 {
                at_least_one_write = false;
            }
        }

        assert!(
            at_least_one_write,
            "all workers need to write at least one byte"
        );
    }
}

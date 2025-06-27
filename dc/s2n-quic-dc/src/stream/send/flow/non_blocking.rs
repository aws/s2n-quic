// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Credits;
use crate::stream::{
    send::{
        error::{self, Error},
        flow,
    },
    TransportFeatures,
};
use atomic_waker::AtomicWaker;
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};
use s2n_quic_core::{ensure, varint::VarInt};
use std::sync::OnceLock;

const ERROR_MASK: u64 = 1 << 63;
const FINISHED_MASK: u64 = 1 << 62;
const OFFSET_MASK: u64 = !(ERROR_MASK | FINISHED_MASK);

pub struct State {
    /// Monotonic offset which tracks where the application is currently writing
    stream_offset: AtomicU64,
    /// Monotonic offset which indicates the maximum offset the application can write to
    flow_offset: AtomicU64,
    /// Notifies an application of newly-available flow credits
    poll_waker: AtomicWaker,
    stream_error: OnceLock<Error>,
    // TODO add a list for the `acquire` future wakers
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("flow::non_blocking::State")
            .field("stream_offset", &self.stream_offset.load(Ordering::Relaxed))
            .field("flow_offset", &self.flow_offset.load(Ordering::Relaxed))
            .finish()
    }
}

impl State {
    #[inline]
    pub fn new(initial_flow_offset: VarInt) -> Self {
        Self {
            stream_offset: AtomicU64::new(0),
            flow_offset: AtomicU64::new(initial_flow_offset.as_u64()),
            poll_waker: AtomicWaker::new(),
            stream_error: OnceLock::new(),
        }
    }
}

impl State {
    #[inline]
    pub fn stream_offset(&self) -> VarInt {
        let value = self.stream_offset.load(Ordering::Relaxed);
        // mask off the two upper bits
        let value = value & OFFSET_MASK;
        unsafe { VarInt::new_unchecked(value) }
    }

    /// Called by the background worker to release flow credits
    ///
    /// Callers MUST ensure the provided offset is monotonic.
    #[inline]
    pub fn release(&self, flow_offset: VarInt) {
        tracing::trace!(release = %flow_offset);
        self.flow_offset
            .store(flow_offset.as_u64(), Ordering::Release);
        self.poll_waker.wake();
    }

    /// Called by the background worker to release flow credits
    ///
    /// This version only releases credits if the value is strictly less than the previous value
    #[inline]
    pub fn release_max(&self, flow_offset: VarInt) {
        tracing::trace!(release = %flow_offset);
        let prev = self
            .flow_offset
            .fetch_max(flow_offset.as_u64(), Ordering::Release);

        // if the flow offset was updated then wake the application waker
        if prev < flow_offset.as_u64() {
            self.poll_waker.wake();
        }
    }

    #[inline]
    pub fn set_error(&self, error: Error) {
        let _ = self.stream_error.set(error);
        self.stream_offset.fetch_or(ERROR_MASK, Ordering::Relaxed);
        self.poll_waker.wake();
    }

    /// Called by the application to acquire flow credits
    #[inline]
    pub async fn acquire(
        &self,
        request: flow::Request,
        features: &TransportFeatures,
    ) -> Result<Credits, Error> {
        core::future::poll_fn(|cx| self.poll_acquire(cx, request, features)).await
    }

    /// Called by the application to acquire flow credits
    #[inline]
    pub fn poll_acquire(
        &self,
        cx: &mut Context,
        mut request: flow::Request,
        features: &TransportFeatures,
    ) -> Poll<Result<Credits, Error>> {
        let mut current_offset = self.acquire_offset(&request)?;

        let mut stored_waker = false;

        loop {
            let flow_offset = self.flow_offset.load(Ordering::Acquire);

            let Some(flow_credits) = flow_offset
                .checked_sub(current_offset & OFFSET_MASK)
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
                current_offset = self.acquire_offset(&request)?;

                continue;
            };

            if !features.is_flow_controlled() {
                // clamp the request to the flow credits we have
                request.clamp(flow_credits);
            }

            let mut new_offset = (current_offset & OFFSET_MASK)
                .checked_add(request.len as u64)
                .filter(|v| *v <= VarInt::MAX.as_u64())
                .ok_or_else(|| error::Kind::PayloadTooLarge.err())?;

            // record that we've sent the final offset
            if request.is_fin || current_offset & FINISHED_MASK == FINISHED_MASK {
                new_offset |= FINISHED_MASK;
            }

            let result = self.stream_offset.compare_exchange(
                current_offset,
                new_offset,
                Ordering::Release, // TODO is this the correct ordering?
                Ordering::Acquire,
            );

            match result {
                Ok(_) => {
                    // the offset was correctly updated so return our acquired credits
                    let acquired_offset =
                        unsafe { VarInt::new_unchecked(current_offset & OFFSET_MASK) };
                    let credits = request.response(acquired_offset);
                    return Poll::Ready(Ok(credits));
                }
                Err(updated_offset) => {
                    // the offset was updated from underneath us so try again
                    current_offset = self.process_offset(updated_offset, &request)?;
                    // clear the fact that we stored the waker, since we need to do a full sync
                    // to get the correct state
                    stored_waker = false;
                    continue;
                }
            }
        }
    }

    #[inline]
    fn acquire_offset(&self, request: &flow::Request) -> Result<u64, Error> {
        self.process_offset(self.stream_offset.load(Ordering::Acquire), request)
    }

    #[inline]
    fn process_offset(&self, offset: u64, request: &flow::Request) -> Result<u64, Error> {
        if offset & ERROR_MASK == ERROR_MASK {
            let error = self
                .stream_error
                .get()
                .copied()
                .unwrap_or_else(|| error::Kind::FatalError.err());
            return Err(error);
        }

        if offset & FINISHED_MASK == FINISHED_MASK {
            ensure!(request.len == 0, Err(error::Kind::FinalSizeChanged.err()));
        }

        Ok(offset)
    }
}

#[cfg(test)]
mod tests {
    use bolero::check;

    use super::*;
    use crate::stream::send::path;
    use std::sync::Arc;

    #[tokio::test]
    async fn concurrent_flow() {
        let mut initial_offset = VarInt::from_u8(255);
        let expected_len = VarInt::from_u16(u16::MAX);
        let state = Arc::new(State::new(initial_offset));
        let path_info = path::Info {
            max_datagram_size: 1500,
            send_quantum: 10,
            ecn: Default::default(),
            next_expected_control_packet: Default::default(),
        };
        let total = Arc::new(AtomicU64::new(0));
        // TODO support more than one Waker via intrusive list or something
        let workers = 1;
        let worker_counts = Vec::from_iter((0..workers).map(|_| Arc::new(AtomicU64::new(0))));
        let features = TransportFeatures::UDP;

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
                        initial_len: buffer_len,
                        is_fin,
                    };
                    request.clamp(path_info.max_flow_credits(max_header_len, max_segments));

                    let Ok(credits) = state.acquire(request, &features).await else {
                        break;
                    };

                    println!(
                        "thread={idx} offset={}..{} is_fin={}",
                        credits.offset,
                        credits.offset + credits.len,
                        credits.is_fin,
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
                        // we already wrote our fin
                        if is_fin {
                            break;
                        }
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
            println!("thread={idx}, count={count}");
            if count == 0 {
                at_least_one_write = false;
            }
        }

        assert!(
            at_least_one_write,
            "all workers need to write at least one byte"
        );
    }

    #[test]
    fn error_test() {
        check!()
            .with_type::<(u8, u8, bool)>()
            .cloned()
            .for_each(|(initial_offset, len, is_fin)| {
                let state = Arc::new(State::new(VarInt::from_u8(initial_offset)));

                state.set_error(Error::new(error::Kind::FatalError));

                let len = len as _;
                let request = flow::Request {
                    len,
                    initial_len: len,
                    is_fin,
                };

                state.acquire_offset(&request).unwrap_err();
            })
    }
}

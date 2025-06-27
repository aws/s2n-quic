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
use s2n_quic_core::{ensure, varint::VarInt};
use std::sync::{Condvar, Mutex};

pub struct State {
    state: Mutex<Inner>,
    notify: Condvar,
}

impl State {
    #[inline]
    pub fn new(initial_flow_offset: VarInt) -> Self {
        Self {
            state: Mutex::new(Inner {
                stream_offset: VarInt::ZERO,
                flow_offset: initial_flow_offset,
                is_finished: false,
            }),
            notify: Condvar::new(),
        }
    }
}

struct Inner {
    /// Monotonic offset which tracks where the application is currently writing
    stream_offset: VarInt,
    /// Monotonic offset which indicates the maximum offset the application can write to
    flow_offset: VarInt,
    /// Indicates that the stream has been finalized
    is_finished: bool,
}

impl State {
    /// Called by the background worker to release flow credits
    ///
    /// Callers MUST ensure the provided offset is monotonic.
    #[inline]
    pub fn release(&self, flow_offset: VarInt) -> Result<(), Error> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| error::Kind::FatalError.err())?;

        // only notify subscribers if we actually increment the offset
        debug_assert!(
            guard.flow_offset < flow_offset,
            "flow offsets MUST be monotonic"
        );
        ensure!(guard.flow_offset < flow_offset, Ok(()));

        guard.flow_offset = flow_offset;
        drop(guard);

        self.notify.notify_all();

        Ok(())
    }

    /// Called by the application to acquire flow credits
    #[inline]
    pub fn acquire(
        &self,
        mut request: flow::Request,
        features: &TransportFeatures,
    ) -> Result<Credits, Error> {
        let mut guard = self
            .state
            .lock()
            .map_err(|_| error::Kind::FatalError.err())?;

        loop {
            ensure!(!guard.is_finished, Err(error::Kind::FinalSizeChanged.err()));

            // TODO check for an error

            let current_offset = guard.stream_offset;
            let flow_offset = guard.flow_offset;

            debug_assert!(
                current_offset <= flow_offset,
                "current_offset={current_offset} should be <= flow_offset={flow_offset}"
            );

            let Some(flow_credits) = flow_offset
                .as_u64()
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
                guard = self
                    .notify
                    .wait(guard)
                    .map_err(|_| error::Kind::FatalError.err())?;
                continue;
            };

            if !features.is_flow_controlled() {
                // clamp the request to the flow credits we have
                request.clamp(flow_credits);
            }

            // update the stream offset with the given request
            guard.stream_offset = current_offset
                .checked_add_usize(request.len)
                .ok_or_else(|| error::Kind::PayloadTooLarge.err())?;

            // update the finished status
            guard.is_finished |= request.is_fin;

            // drop the lock before notifying all of the waiting Condvar
            drop(guard);

            // notify the other handles when we finish
            if request.is_fin {
                self.notify.notify_all();
            }

            // the offset was correctly updated so return our acquired credits
            let credits = request.response(current_offset);

            return Ok(credits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::send::path;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    #[test]
    fn concurrent_flow() {
        let mut initial_offset = VarInt::from_u8(255);
        let expected_len = VarInt::from_u16(u16::MAX);
        let state = State::new(initial_offset);
        let path_info = path::Info {
            max_datagram_size: 1500,
            send_quantum: 10,
            ecn: Default::default(),
            next_expected_control_packet: Default::default(),
        };
        let total = AtomicU64::new(0);
        let workers = 5;
        let worker_counts = Vec::from_iter((0..workers).map(|_| AtomicU64::new(0)));
        let features = TransportFeatures::UDP;

        thread::scope(|s| {
            let total = &total;
            let path_info = &path_info;
            let state = &state;

            for (idx, count) in worker_counts.iter().enumerate() {
                s.spawn(move || {
                    thread::sleep(core::time::Duration::from_millis(10));

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

                        let Ok(credits) = state.acquire(request, &features) else {
                            break;
                        };

                        eprintln!(
                            "thread={idx} offset={}..{}",
                            credits.offset,
                            credits.offset + credits.len
                        );
                        buffer_len += 1;
                        buffer_len = buffer_len.min(
                            expected_len
                                .as_u64()
                                .saturating_sub(credits.offset.as_u64())
                                .saturating_sub(credits.len as u64)
                                as usize,
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

            s.spawn(|| {
                let mut credits = 10;
                while initial_offset < expected_len {
                    thread::sleep(core::time::Duration::from_millis(1));
                    initial_offset = (initial_offset + credits).min(expected_len);
                    credits += 1;
                    let _ = state.release(initial_offset);
                }
            });
        });

        assert_eq!(total.load(Ordering::Relaxed), expected_len.as_u64());
        let mut at_least_one_write = true;
        for (idx, count) in worker_counts.into_iter().enumerate() {
            let count = count.load(Ordering::Relaxed);
            eprintln!("thread={idx}, count={count}");
            if count == 0 {
                at_least_one_write = false;
            }
        }

        let _ = at_least_one_write;

        // TODO the Mutex mechanism doesn't fairly distribute between workers so don't make this
        // assertion until we can do something more reliable
        /*
        assert!(
            at_least_one_write,
            "all workers need to write at least one byte"
        );
        */
    }
}

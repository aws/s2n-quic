// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::socket::pool::descriptor::{Recycler, Unfilled};
use core::fmt;

pub mod descriptor;

#[derive(Clone)]
pub struct Pool {
    max_packet_size: u16,
}

impl fmt::Debug for Pool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Pool")
            .field("max_packet_size", &self.max_packet_size)
            .finish()
    }
}

impl Pool {
    /// Creates a pool with the given `max_packet_size`.
    ///
    /// # Notes
    ///
    /// `max_packet_size` does not account for GRO capabilities of the underlying socket. If
    /// GRO is enabled, the `max_packet_size` should be set to `u16::MAX`.
    #[inline]
    pub fn new(max_packet_size: u16) -> Self {
        Pool { max_packet_size }
    }

    /// Allocates a new unfilled packet.
    ///
    /// Returns `None` if the packet allocator is exhausted (backpressure signal).
    #[inline]
    pub fn alloc<R: Recycler>(&self) -> Option<Unfilled<R>> {
        Unfilled::new(self.max_packet_size)
    }

    /// Allocates a new unfilled packet with a recycler attached.
    ///
    /// When the descriptor is eventually dropped, it will be pushed back to the
    /// recycling channel instead of being deallocated.
    #[inline]
    pub fn alloc_with_recycler<R: Recycler + Clone>(&self, recycler: &R) -> Option<Unfilled<R>> {
        Unfilled::new_with_recycler(self.max_packet_size, recycler.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        intrusive,
        socket::{
            channel::{intrusive::sync, Budget, Receiver as _},
            pool::descriptor::{Filled, RecycleAdapter, SyncRecycler},
        },
        testing::{ext::*, sim},
    };
    use bolero::{check, TypeGenerator};
    use std::{
        collections::VecDeque,
        net::{Ipv4Addr, SocketAddr},
    };

    #[derive(TypeGenerator, Debug)]
    enum Op {
        Alloc,
        DropUnfilled {
            idx: u8,
        },
        Fill {
            idx: u8,
            port: u8,
            segment_count: u8,
            segment_len: u8,
        },
        DropFilled {
            idx: u8,
        },
    }

    struct Model {
        pool: Pool,
        unfilled: VecDeque<Unfilled>,
        filled: VecDeque<Filled>,
    }

    impl Model {
        fn new(max_packet_size: u16) -> Self {
            let pool = Pool::new(max_packet_size);
            Self {
                pool,
                unfilled: VecDeque::new(),
                filled: VecDeque::new(),
            }
        }

        fn alloc(&mut self) {
            if let Some(desc) = self.pool.alloc::<SyncRecycler>() {
                self.unfilled.push_back(desc);
            }
        }

        fn drop_unfilled(&mut self, idx: usize) {
            if self.unfilled.is_empty() {
                return;
            }

            let idx = idx % self.unfilled.len();
            let _ = self.unfilled.remove(idx).unwrap();
        }

        fn drop_filled(&mut self, idx: usize) {
            if self.filled.is_empty() {
                return;
            }
            let idx = idx % self.filled.len();
            let _ = self.filled.remove(idx).unwrap();
        }

        fn fill(&mut self, idx: usize, port: u16, segment_count: u8, segment_len: u8) {
            let Self {
                unfilled, filled, ..
            } = self;

            if unfilled.is_empty() {
                return;
            }
            let idx = idx % unfilled.len();
            let unfilled = unfilled.remove(idx).unwrap();

            let src = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);

            let segment_len = segment_len as usize;
            let segment_count = segment_count as usize;
            let mut actual_segment_count = 0;

            let res = unfilled.fill_with(|addr, cmsg, mut payload| {
                if port == 0 {
                    return Err(());
                }

                addr.set(src.into());

                if segment_count > 1 {
                    cmsg.set_segment_len(segment_len as _);
                }
                let mut offset = 0;

                for segment_idx in 0..segment_count {
                    let remaining = &mut payload[offset..];
                    let len = remaining.len().min(segment_len);
                    if len == 0 {
                        break;
                    }

                    actual_segment_count += 1;
                    remaining[..len].fill(segment_idx as u8);
                    offset += len;
                }

                Ok(offset)
            });

            assert_eq!(res.is_err(), port == 0);

            if let Ok(segments) = res {
                for (idx, segment) in segments.into_iter().enumerate() {
                    // we allow only one segment to be empty. this makes it easier to log when we get empty packets, which are unexpected
                    if segment.is_empty() {
                        assert_eq!(actual_segment_count, 0);
                        assert_eq!(idx, 0);
                        continue;
                    }

                    assert!(
                        idx < actual_segment_count,
                        "{idx} < {actual_segment_count}, {:?}",
                        segment.payload()
                    );

                    //  the final segment is allowed to be undersized
                    if idx == actual_segment_count - 1 {
                        assert!(segment.len() as usize <= segment_len);
                    } else {
                        assert_eq!(segment.len() as usize, segment_len);
                    }

                    // make sure bytes match the segment pattern
                    for byte in segment.payload().iter() {
                        assert_eq!(*byte, idx as u8);
                    }

                    filled.push_back(segment);
                }
            }
        }

        fn apply(&mut self, op: &Op) {
            match op {
                Op::Alloc => self.alloc(),
                Op::DropUnfilled { idx } => self.drop_unfilled(*idx as usize),
                Op::Fill {
                    idx,
                    port,
                    segment_count,
                    segment_len,
                } => self.fill(*idx as _, *port as _, *segment_count, *segment_len),
                Op::DropFilled { idx } => self.drop_filled(*idx as usize),
            }
        }
    }

    #[test]
    fn model_test() {
        check!()
            .with_type::<Vec<Op>>()
            .with_test_time(core::time::Duration::from_secs(20))
            .for_each(|ops| {
                let max_packet_size = 1000;
                let mut model = Model::new(max_packet_size);
                for op in ops {
                    model.apply(op);
                }
            });
    }

    #[test]
    fn descriptor_recycles_through_channel() {
        sim(|| {
            async {
                let (tx, mut rx) = sync::new_with_adapter::<RecycleAdapter<SyncRecycler>>();
                let weak = SyncRecycler(tx.downgrade());
                let pool = Pool::new(1500);

                // Allocate with recycler, fill, then drop
                let unfilled = pool.alloc_with_recycler(&weak).unwrap();
                let segments = unfilled
                    .fill_with(|addr, _cmsg, mut iov| {
                        iov[..4].copy_from_slice(b"test");
                        addr.set(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 1234).into());
                        Ok::<_, std::io::Error>(4)
                    })
                    .unwrap();
                let filled = segments.take_filled();
                assert_eq!(filled.payload(), b"test");
                drop(filled);

                // The descriptor should now be in the channel — recv it
                let mut budget = Budget::new(16);
                let batch: Option<intrusive::List<RecycleAdapter<SyncRecycler>>> =
                    rx.recv(&mut budget).await;
                let list = batch.expect("channel should have a batch");
                assert_eq!(list.len(), 1, "expected exactly 1 recycled descriptor");

                drop(tx);
            }
            .primary()
            .spawn();
        });
    }

    #[test]
    fn descriptor_deallocs_without_recycler() {
        sim(|| {
            async {
                let (tx, mut rx) = sync::new_with_adapter::<RecycleAdapter<SyncRecycler>>();
                let weak = SyncRecycler(tx.downgrade());
                let pool = Pool::new(1500);

                // Allocate WITHOUT recycler, fill, then drop — should dealloc, not recycle
                let unfilled = pool.alloc::<SyncRecycler>().unwrap();
                let segments = unfilled
                    .fill_with(|addr, _cmsg, mut iov| {
                        iov[..4].copy_from_slice(b"test");
                        addr.set(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 1234).into());
                        Ok::<_, std::io::Error>(4)
                    })
                    .unwrap();
                drop(segments);

                // Channel should be empty — nothing was recycled.
                // Use a timeout to confirm nothing arrives.
                let result = bach::time::timeout(
                    core::time::Duration::from_millis(10),
                    rx.recv(&mut Budget::new(16)),
                )
                .await;
                assert!(result.is_err(), "expected timeout (empty channel)");

                drop(weak);
                drop(tx);
            }
            .primary()
            .spawn();
        });
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    msg::addr::Addr,
    socket::recv::descriptor::{Descriptor, DescriptorInner, FreeList, Unfilled},
};
use std::{
    alloc::Layout,
    ptr::NonNull,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Pool {
    free: Arc<Free>,
    max_packet_size: u16,
    packet_count: usize,
}

impl Pool {
    /// Creates a pool with the given `max_packet_size` and `packet_count`.
    ///
    /// # Notes
    ///
    /// `max_packet_size` does not account for GRO capabilities of the underlying socket. If
    /// GRO is enabled, the `max_packet_size` should be set to `u16::MAX`.
    #[inline]
    pub fn new(max_packet_size: u16, packet_count: usize) -> Self {
        let free = Free::new(packet_count);
        let mut pool = Pool {
            free,
            max_packet_size,
            packet_count,
        };
        pool.grow();
        pool
    }

    #[inline]
    pub fn alloc(&self) -> Option<Unfilled> {
        self.free.alloc()
    }

    #[inline]
    pub fn alloc_or_grow(&mut self) -> Unfilled {
        loop {
            if let Some(descriptor) = self.free.alloc() {
                return descriptor;
            }
            self.grow();
        }
    }

    #[inline(never)] // this should happen rarely
    fn grow(&mut self) {
        let (region, layout) = Region::alloc(self.max_packet_size, self.packet_count);

        let ptr = region.ptr;
        let packet = layout.packet;
        let addr_offset = layout.addr_offset;
        let packet_offset = layout.packet_offset;
        let max_packet_size = layout.max_packet_size;

        let mut pending = vec![];

        for idx in 0..self.packet_count {
            let offset = packet.size() * idx;
            unsafe {
                let descriptor = ptr.as_ptr().add(offset).cast::<DescriptorInner>();
                let addr = ptr.as_ptr().add(offset + addr_offset).cast::<Addr>();
                let data = ptr.as_ptr().add(offset + packet_offset);

                // `data` pointer is already zeroed out with the initial allocation
                // initialize the address
                addr.write(Addr::default());
                // initialize the descriptor - note that it is self-referential to `addr`, `data`, and `free`
                // SAFETY: address, payload, and memory are all initialized
                descriptor.write(DescriptorInner::new(
                    idx as _,
                    max_packet_size,
                    NonNull::new_unchecked(addr),
                    NonNull::new_unchecked(data),
                    self.free.clone(),
                ));

                // push the descriptor into the free list
                let descriptor = Descriptor::new(NonNull::new_unchecked(descriptor));
                pending.push(descriptor);
            }
        }

        self.free.record_region(region, pending);
    }
}

impl Drop for Pool {
    #[inline]
    fn drop(&mut self) {
        let _ = self.free.close();
    }
}

struct Region {
    ptr: NonNull<u8>,
    layout: Layout,
}

struct RegionLayout {
    packet: Layout,
    addr_offset: usize,
    packet_offset: usize,
    max_packet_size: u16,
}

unsafe impl Send for Region {}
unsafe impl Sync for Region {}

impl Region {
    #[inline]
    fn alloc(mut max_packet_size: u16, packet_count: usize) -> (Self, RegionLayout) {
        debug_assert!(max_packet_size > 0, "packets need to be at least 1 byte");
        debug_assert!(packet_count > 0, "there needs to be at least 1 packet");

        // first create the descriptor layout
        let descriptor = Layout::new::<DescriptorInner>();
        // extend it with the address value
        let (header, addr_offset) = descriptor.extend(Layout::new::<Addr>()).unwrap();
        // finally place the packet data at the end
        let (packet, packet_offset) = header
            .extend(Layout::array::<u8>(max_packet_size as usize).unwrap())
            .unwrap();

        // add any extra padding we need
        let without_padding_len = packet.size();
        let packet = packet.pad_to_align();

        // if we needed to add padding then use that for the packet buffer since it will go to waste otherwise
        let padding_len = packet.size() - without_padding_len;
        max_packet_size = max_packet_size.saturating_add(padding_len as u16);

        let packets = {
            // TODO use `packet.repeat(packet_count)` once stable
            // https://doc.rust-lang.org/stable/core/alloc/struct.Layout.html#method.repeat
            Layout::from_size_align(
                packet.size().checked_mul(packet_count).unwrap(),
                packet.align(),
            )
            .unwrap()
        };

        let ptr = unsafe {
            // SAFETY: the layout is non-zero size
            debug_assert_ne!(packets.size(), 0);
            // ensure that the allocation is zeroed out so we don't have to worry about MaybeUninit
            std::alloc::alloc_zeroed(packets)
        };
        let ptr = NonNull::new(ptr).expect("failed to allocate memory");

        let region = Self {
            ptr,
            layout: packets,
        };

        let layout = RegionLayout {
            packet,
            addr_offset,
            packet_offset,
            max_packet_size,
        };

        (region, layout)
    }
}

impl Drop for Region {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr.as_ptr(), self.layout);
        }
    }
}

/// A free list of unfilled descriptors
///
/// Note that this uses a [`Vec`] instead of [`std::collections::VecDeque`], which acts more
/// like a stack than a queue. This is to prefer more-recently used descriptors which should
/// hopefully reduce the number of cache misses.
struct Free(Mutex<FreeInner>);

impl Free {
    #[inline]
    fn new(packet_count: usize) -> Arc<Self> {
        let descriptors = Vec::with_capacity(packet_count);
        let regions = Vec::with_capacity(1);
        let inner = FreeInner {
            descriptors,
            regions,
            total: 0,
            open: true,
        };
        Arc::new(Self(Mutex::new(inner)))
    }

    #[inline]
    fn alloc(&self) -> Option<Unfilled> {
        self.0
            .lock()
            .unwrap()
            .descriptors
            .pop()
            .map(Unfilled::from_descriptor)
    }

    #[inline]
    fn record_region(&self, region: Region, mut descriptors: Vec<Descriptor>) {
        let mut inner = self.0.lock().unwrap();
        inner.regions.push(region);
        inner.total += descriptors.len();
        inner.descriptors.append(&mut descriptors);
        drop(inner);
        drop(descriptors);
    }

    #[inline]
    fn close(&self) -> Option<FreeInner> {
        let mut inner = self.0.lock().unwrap();
        inner.open = false;
        inner.try_free()
    }
}

impl FreeList for Free {
    #[inline]
    fn free(&self, descriptor: Descriptor) -> Option<Box<dyn 'static + Send>> {
        let mut inner = self.0.lock().unwrap();
        inner.descriptors.push(descriptor);
        if inner.open {
            return None;
        }
        inner
            .try_free()
            .map(|to_free| Box::new(to_free) as Box<dyn 'static + Send>)
    }
}

struct FreeInner {
    descriptors: Vec<Descriptor>,
    regions: Vec<Region>,
    total: usize,
    open: bool,
}

impl FreeInner {
    #[inline(never)] // this is rarely called
    fn try_free(&mut self) -> Option<Self> {
        if self.descriptors.len() < self.total {
            return None;
        }

        // move all of the allocations out of itself, since this is self-referential
        Some(core::mem::replace(
            self,
            FreeInner {
                descriptors: Vec::new(),
                regions: Vec::new(),
                total: 0,
                open: false,
            },
        ))
    }
}

impl Drop for FreeInner {
    #[inline]
    fn drop(&mut self) {
        for descriptor in self.descriptors.drain(..) {
            unsafe {
                // SAFETY: the free list is closed and there are no outstanding descriptors
                descriptor.drop_in_place();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{socket::recv::descriptor::Filled, testing::init_tracing};
    use bolero::{check, TypeGenerator};
    use std::{
        collections::{HashMap, VecDeque},
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
        epoch: u64,
        references: HashMap<u64, usize>,
        unfilled: VecDeque<Unfilled>,
        filled: VecDeque<(u64, Filled)>,
        expected_free_packets: usize,
    }

    impl Model {
        fn new(max_packet_size: u16, packet_count: usize) -> Self {
            let pool = Pool::new(max_packet_size, packet_count);
            Self {
                pool,
                epoch: 0,
                references: HashMap::new(),
                unfilled: VecDeque::new(),
                filled: VecDeque::new(),
                expected_free_packets: packet_count,
            }
        }

        fn alloc(&mut self) {
            if let Some(desc) = self.pool.alloc() {
                self.unfilled.push_back(desc);
                self.expected_free_packets -= 1;
            } else {
                assert_eq!(self.expected_free_packets, 0);
            }
        }

        fn drop_unfilled(&mut self, idx: usize) {
            if self.unfilled.is_empty() {
                return;
            }

            let idx = idx % self.unfilled.len();
            let _ = self.unfilled.remove(idx).unwrap();
            self.expected_free_packets += 1;
        }

        fn drop_filled(&mut self, idx: usize) {
            if self.filled.is_empty() {
                return;
            }
            let idx = idx % self.filled.len();
            let (epoch, _descriptor) = self.filled.remove(idx).unwrap();
            let count = self.references.entry(epoch).or_default();
            *count -= 1;
            if *count == 0 {
                self.references.remove(&epoch);
                self.expected_free_packets += 1;
            }
        }

        fn fill(&mut self, idx: usize, port: u16, segment_count: u8, segment_len: u8) {
            let Self {
                epoch,
                references,
                unfilled,
                filled,
                expected_free_packets,
                ..
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

            let res = unfilled.recv_with(|addr, cmsg, mut payload| {
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
                if actual_segment_count > 0 {
                    references.insert(*epoch, actual_segment_count);
                }

                for (idx, segment) in segments.enumerate() {
                    // we allow only one segment to be empty. this makes it easier to log when we get empty packets, which are unexpected
                    if segment.is_empty() {
                        assert_eq!(actual_segment_count, 0);
                        assert_eq!(idx, 0);
                        *expected_free_packets += 1;
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

                    filled.push_back((*epoch, segment));
                }

                *epoch += 1;
            } else {
                *expected_free_packets += 1;
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
        init_tracing();

        check!()
            .with_type::<Vec<Op>>()
            .with_test_time(core::time::Duration::from_secs(20))
            .for_each(|ops| {
                let max_packet_size = 1000;
                let expected_free_packets = 16;
                let mut model = Model::new(max_packet_size, expected_free_packets);
                for op in ops {
                    model.apply(op);
                }
            });
    }
}

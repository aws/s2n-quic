use crate::{
    clock::precision,
    msg::{self, addr::Addr},
    socket::{pool::descriptor, send::completion::Completion},
    sync::intrusive_queue as queue,
};
use core::fmt;
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{collections::VecDeque, io::IoSlice, ops::RangeBounds};

pub type Entry<Info, Meta, Completion> = queue::Entry<Transmission<Info, Meta, Completion>>;

#[derive(Debug)]
pub struct Transmission<Info, Meta, Completion> {
    pub descriptors: crate::intrusive_queue::Queue<(descriptor::Filled, Info)>,
    pub total_len: u16,
    pub meta: Meta,
    pub transmission_time: Option<precision::Timestamp>,
    pub completion: Completion,
}

impl<Info, Meta, Completion> crate::socket::send::wheel::Scheduled
    for Transmission<Info, Meta, Completion>
{
    fn transmission_time(&self) -> Option<precision::Timestamp> {
        self.transmission_time
    }

    fn set_transmission_time(&mut self, time: precision::Timestamp) {
        self.transmission_time = Some(time);
    }
}

impl<Info, Meta, Completion> crate::socket::channel::ByteCost for Transmission<Info, Meta, Completion> {
    fn byte_cost(&self) -> u64 {
        self.total_len as u64
    }
}

impl<Info, M, C> crate::socket::channel::Sendable for Transmission<Info, M, C>
where
    C: Completion<Info, M>,
{
    fn send<S: crate::socket::send::Socket>(
        &mut self,
        socket: &S,
    ) -> std::io::Result<()> {
        self.send_with(|addr, ecn, ioslices| {
            // Get segment size from first descriptor
            let segment_len = ioslices.first().map(|s| s.len()).unwrap_or(0) as u16;

            socket.send_msg(addr, ioslices, segment_len, ecn)?;
            Ok(())
        })
    }
}

impl<Info, M, C> Transmission<Info, M, C>
where
    C: Completion<Info, M>,
{
    pub fn send_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Addr, ExplicitCongestionNotification, &[IoSlice]) -> R,
    {
        debug_assert!(!self.descriptors.is_empty());
        debug_assert!(self.descriptors.len() <= msg::segment::MAX_COUNT);

        let mut segments = [IoSlice::new(&[]); msg::segment::MAX_COUNT];
        let segments = &mut segments[..self.descriptors.len()];

        let first = &self
            .descriptors
            .front()
            .expect("missing first descriptor")
            .0;
        let addr = first.remote_address();
        let ecn = first.ecn();
        let segment_len = first.payload().len();

        let mut count = 0;
        for ((segment, _info), ioslice) in self.descriptors.iter().zip(segments.iter_mut()) {
            debug_assert_eq!(segment.remote_address(), addr);
            debug_assert_eq!(segment.ecn(), ecn);
            let payload = segment.payload();
            *ioslice = IoSlice::new(payload);
            count += 1;
            // The last segment can be undersized
            if count == self.descriptors.len() {
                debug_assert!(payload.len() <= segment_len);
            } else {
                debug_assert_eq!(payload.len(), segment_len);
            }
        }

        f(addr, ecn, segments)
    }
}

struct Batch<Info, Meta, Completion> {
    entry: Entry<Info, Meta, Completion>,
    application_len: u16,
}

pub struct Builder<Info, Meta, Completion> {
    batches: VecDeque<Batch<Info, Meta, Completion>>,
}

impl<Info, Meta, Completion> Default for Builder<Info, Meta, Completion> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Info, Meta, Completion> fmt::Debug for Builder<Info, Meta, Completion> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Builder")
            .field("batches", &self.batches.len())
            .finish()
    }
}

impl<Info, Meta, Completion> Builder<Info, Meta, Completion> {
    pub fn new() -> Self {
        Self {
            batches: VecDeque::with_capacity(2),
        }
    }

    pub fn len(&self) -> usize {
        self.batches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    pub fn append(&mut self, other: &mut Self) {
        self.batches.append(&mut other.batches);
    }

    pub fn push_segment(
        &mut self,
        info: Info,
        meta: Meta,
        application_len: u16,
        descriptor: descriptor::Filled,
        max_segments: usize,
        transmission_alloc: impl Fn() -> Entry<Info, Meta, Completion>,
    ) {
        // Clamp max_segments to the maximum number of segments allowed per
        // sendmsg call. Callers may pass a higher value (e.g. from GSO
        // configuration) that exceeds the iovec/total-payload limits.
        let max_segments = max_segments.min(msg::segment::MAX_COUNT);

        let batch = loop {
            if let Some(batch) = self.batches.back_mut() {
                let mut can_push = true;

                can_push &= batch.entry.descriptors.len() < max_segments;

                debug_assert!(descriptor.len() <= msg::segment::MAX_TOTAL);
                can_push &= (batch.entry.total_len as usize + descriptor.len() as usize)
                    <= msg::segment::MAX_TOTAL as usize;

                if let Some((first, _)) = batch.entry.descriptors.front() {
                    // We can push as long as our current message isn't greater than the segment size
                    can_push &= first.len() >= descriptor.len();

                    can_push &= first.remote_address() == descriptor.remote_address();
                    can_push &= first.ecn() == descriptor.ecn();

                    let (last, _) = batch.entry.descriptors.back().unwrap();

                    // We can push as long as the last message isn't undersized
                    can_push &= first.len() == last.len();
                }

                if can_push {
                    break batch;
                }
            }

            let entry = transmission_alloc();
            self.batches.push_back(Batch {
                entry,
                application_len: 0,
            });
        };

        batch.entry.total_len += descriptor.len();
        batch
            .entry
            .descriptors
            .push_back(queue::Entry::new((descriptor, info)));
        // use the last provided meta value
        batch.entry.meta = meta;

        batch.application_len += application_len;
    }

    pub fn pop_front(&mut self) -> Option<(Entry<Info, Meta, Completion>, u16)> {
        let batch = self.batches.pop_front()?;
        Some((batch.entry, batch.application_len))
    }

    pub fn push_front(&mut self, entry: Entry<Info, Meta, Completion>, application_len: u16) {
        self.batches.push_front(Batch {
            entry,
            application_len,
        });
    }

    pub fn drain(
        &mut self,
        range: impl RangeBounds<usize>,
    ) -> impl Iterator<Item = (Entry<Info, Meta, Completion>, u16)> + '_ {
        self.batches
            .drain(range)
            .map(|batch| (batch.entry, batch.application_len))
    }

    pub fn clear_head(&mut self, count: usize) {
        self.batches.drain(..count);
    }
}

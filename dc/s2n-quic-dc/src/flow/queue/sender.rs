// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{free_list, handle::Sender, inner::Error, queue_id};
use s2n_quic_core::varint::VarInt;
use std::sync::{Arc, RwLock};

pub struct State<S: 'static, C: 'static, Key: 'static> {
    pub(super) pages: RwLock<SenderPages<S, C, Key>>,
}

impl<S: 'static, C: 'static, Key: 'static> State<S, C, Key> {
    #[inline]
    pub fn new(epoch: usize) -> Arc<Self> {
        Arc::new(Self {
            pages: RwLock::new(SenderPages::new(epoch)),
        })
    }
}

pub struct Senders<S: 'static, C: 'static, Key: 'static, const INITIAL_PAGE_SIZE: usize> {
    pub(super) state: Arc<State<S, C, Key>>,
    pub(super) local: Vec<Arc<[Sender<S, C, Key>]>>,
    pub(super) memory_handle: Arc<free_list::Memory<S, C, Key>>,
}

impl<S: 'static, C: 'static, Key: 'static, const INITIAL_PAGE_SIZE: usize> Clone
    for Senders<S, C, Key, INITIAL_PAGE_SIZE>
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            memory_handle: self.memory_handle.clone(),
            local: self.local.clone(),
        }
    }
}

impl<S: 'static, C: 'static, Key: 'static, const INITIAL_PAGE_SIZE: usize> Senders<S, C, Key, INITIAL_PAGE_SIZE> {
    #[inline]
    fn refresh_pages(&mut self) {
        let Ok(senders) = self.state.pages.read() else {
            return;
        };

        if self.local.len() == senders.pages.len() {
            return;
        }

        self.local
            .extend_from_slice(&senders.pages[self.local.len()..]);
    }

    /// Computes the page index and intra-page offset for a slot in O(1).
    ///
    /// Pages grow exponentially: page `n` has `INITIAL_PAGE_SIZE * 2^n` slots and
    /// starts at slot `(2^n - 1) * INITIAL_PAGE_SIZE`.  Given a slot `s`:
    ///
    /// ```text
    /// k         = s / INITIAL_PAGE_SIZE
    /// page_idx  = floor(log2(k + 1))          -- highest set bit of (k+1)
    /// page_start = (2^page_idx - 1) * INITIAL_PAGE_SIZE
    /// offset    = s - page_start
    /// ```
    #[inline]
    fn find_page(slot: usize) -> (usize, usize) {
        let k = slot / INITIAL_PAGE_SIZE;
        let page_idx = (k + 1).ilog2() as usize;
        let page_start = ((1usize << page_idx) - 1) * INITIAL_PAGE_SIZE;
        (page_idx, slot - page_start)
    }

    #[inline]
    pub fn lookup<T, F, V>(&mut self, queue_id: VarInt, entry: T, f: F) -> Result<V, Error<T>>
    where
        F: FnOnce(&Sender<S, C, Key>, T) -> Result<V, Error<T>>,
    {
        let slot = queue_id::index(queue_id);
        let (page_idx, offset) = Self::find_page(slot);

        if self.local.len() <= page_idx {
            self.refresh_pages();
            if self.local.len() <= page_idx {
                return Err(Error::Unallocated(entry));
            }
        }

        let Some(page) = self.local.get(page_idx) else {
            return Err(Error::Unallocated(entry));
        };
        let Some(sender) = page.get(offset) else {
            return Err(Error::Unallocated(entry));
        };

        let Some(current_queue_id) = sender.try_queue_id() else {
            return Err(Error::Unallocated(entry));
        };

        if current_queue_id != queue_id {
            return Err(Error::Unallocated(entry));
        }

        f(sender, entry)
    }

    #[inline]
    /// Iterates every currently known sender page entry and invokes `f`.
    ///
    /// # Performance
    ///
    /// This is intentionally expensive: it performs an O(total_queues) walk across
    /// the entire sender table and should only be used for rare control-plane fanout
    /// operations (for example, credential-wide invalidations). Never call this on
    /// hot data-path packet/frame processing.
    pub fn for_each_sender(&mut self, mut f: impl FnMut(&Sender<S, C, Key>)) {
        self.refresh_pages();

        for page in &self.local {
            for sender in page.iter() {
                f(sender);
            }
        }
    }
}

pub(super) struct SenderPages<S: 'static, C: 'static, Key: 'static> {
    pub(super) pages: Vec<Arc<[Sender<S, C, Key>]>>,
    pub(super) epoch: usize,
}

impl<S: 'static, C: 'static, Key: 'static> SenderPages<S, C, Key> {
    #[inline]
    pub(super) fn new(epoch: usize) -> Self {
        Self {
            pages: Vec::with_capacity(8),
            epoch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    /// Oracle for `find_page`: walks pages linearly so it does not share any
    /// code with the O(1) implementation under test.
    fn oracle_find_page<const PS: usize>(slot: usize) -> (usize, usize) {
        let mut base = 0usize;
        let mut page_idx = 0usize;
        loop {
            let page_size = PS * (1usize << page_idx);
            let end = base + page_size;
            if slot < end {
                return (page_idx, slot - base);
            }
            base = end;
            page_idx += 1;
        }
    }

    #[test]
    fn bolero_find_page_round_trip() {
        // Use the configured initial page size so the same value is baked
        // into both the O(1) implementation and the oracle.
        const PS: usize = super::super::INITIAL_PAGE_SIZE;

        check!().with_type::<u32>().for_each(|raw_slot| {
            let slot = (*raw_slot as usize) % super::super::queue_id::MAX_SLOTS;

            let (page_idx, offset) = Senders::<(), (), (), PS>::find_page(slot);

            // Round-trip: reconstructed slot must equal the original.
            let page_start = ((1usize << page_idx) - 1) * PS;
            assert_eq!(
                page_start + offset,
                slot,
                "round-trip failed for slot={slot}"
            );

            // Offset must lie within the page's allocated capacity.
            let page_capacity = PS * (1usize << page_idx);
            assert!(
                offset < page_capacity,
                "offset={offset} out of bounds for page {page_idx} (capacity {page_capacity})"
            );

            // Must agree with the linear-scan oracle.
            assert_eq!(
                (page_idx, offset),
                oracle_find_page::<PS>(slot),
                "O(1) result differs from oracle for slot={slot}"
            );
        });
    }
}

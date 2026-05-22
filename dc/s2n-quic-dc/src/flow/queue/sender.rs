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

pub struct Senders<S: 'static, C: 'static, Key: 'static, const PAGE_SIZE: usize> {
    pub(super) state: Arc<State<S, C, Key>>,
    pub(super) local: Vec<Arc<[Sender<S, C, Key>]>>,
    pub(super) memory_handle: Arc<free_list::Memory<S, C, Key>>,
}

impl<S: 'static, C: 'static, Key: 'static, const PAGE_SIZE: usize> Clone
    for Senders<S, C, Key, PAGE_SIZE>
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            memory_handle: self.memory_handle.clone(),
            local: self.local.clone(),
        }
    }
}

impl<S: 'static, C: 'static, Key: 'static, const PAGE_SIZE: usize> Senders<S, C, Key, PAGE_SIZE> {
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

    #[inline]
    pub fn lookup<T, F, V>(&mut self, queue_id: VarInt, entry: T, f: F) -> Result<V, Error<T>>
    where
        F: FnOnce(&Sender<S, C, Key>, T) -> Result<V, Error<T>>,
    {
        let slot = queue_id::index(queue_id);
        let page = slot / PAGE_SIZE;
        let offset = slot % PAGE_SIZE;

        if self.local.len() <= page {
            self.refresh_pages();
            if self.local.len() <= page {
                return Err(Error::Unallocated(entry));
            }
        }

        let Some(page) = self.local.get(page) else {
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

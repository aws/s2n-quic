// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{free_list, handle::Sender};
use s2n_quic_core::varint::VarInt;
use std::{
    hash::Hash,
    sync::{Arc, RwLock},
};

pub struct State<T: 'static, Key: 'static> {
    pub(super) pages: RwLock<SenderPages<T, Key>>,
}

impl<T: 'static, Key: 'static> State<T, Key>
where
    Key: Eq + Hash,
{
    #[inline]
    pub fn new(epoch: VarInt) -> Arc<Self> {
        Arc::new(Self {
            pages: RwLock::new(SenderPages::new(epoch)),
        })
    }
}

pub struct Senders<T: 'static, Key: 'static, const PAGE_SIZE: usize> {
    pub(super) state: Arc<State<T, Key>>,
    pub(super) local: Vec<Arc<[Sender<T, Key>]>>,
    pub(super) memory_handle: Arc<free_list::Memory<T, Key>>,
    pub(super) base: VarInt,
}

impl<T: 'static, Key: 'static, const PAGE_SIZE: usize> Clone for Senders<T, Key, PAGE_SIZE> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            memory_handle: self.memory_handle.clone(),
            local: self.local.clone(),
            base: self.base,
        }
    }
}

impl<T: 'static, Key: 'static, const PAGE_SIZE: usize> Senders<T, Key, PAGE_SIZE> {
    #[inline]
    pub fn lookup<F: FnOnce(&Sender<T, Key>)>(&mut self, queue_id: VarInt, f: F) {
        let Some(queue_id) = queue_id.checked_sub(self.base) else {
            return;
        };
        let queue_id = queue_id.as_u64() as usize;
        let page = queue_id / PAGE_SIZE;
        let offset = queue_id % PAGE_SIZE;

        if self.local.len() <= page {
            let Ok(senders) = self.state.pages.read() else {
                return;
            };

            // the senders haven't been updated
            if self.local.len() == senders.pages.len() {
                return;
            }

            self.local
                .extend_from_slice(&senders.pages[self.local.len()..]);
        }

        let Some(page) = self.local.get(page) else {
            return;
        };
        let Some(sender) = page.get(offset) else {
            return;
        };
        f(sender)
    }
}

pub(super) struct SenderPages<T: 'static, Key: 'static> {
    pub(super) pages: Vec<Arc<[Sender<T, Key>]>>,
    pub(super) epoch: VarInt,
}

impl<T: 'static, Key: 'static> SenderPages<T, Key> {
    #[inline]
    pub(super) fn new(epoch: VarInt) -> Self {
        Self {
            pages: Vec::with_capacity(8),
            epoch,
        }
    }
}

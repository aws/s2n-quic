// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{free_list, handle::Sender};
use s2n_quic_core::varint::VarInt;
use std::sync::{Arc, RwLock};

pub struct Senders<T: 'static, const PAGE_SIZE: usize> {
    pub(super) senders: Arc<RwLock<SenderPages<T>>>,
    pub(super) local: Vec<Arc<[Sender<T>]>>,
    pub(super) memory_handle: Arc<free_list::Memory<T>>,
}

impl<T: 'static, const PAGE_SIZE: usize> Clone for Senders<T, PAGE_SIZE> {
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
            memory_handle: self.memory_handle.clone(),
            local: self.local.clone(),
        }
    }
}

impl<T: 'static, const PAGE_SIZE: usize> Senders<T, PAGE_SIZE> {
    #[inline]
    pub fn lookup<F: FnOnce(&Sender<T>)>(&mut self, queue_id: VarInt, f: F) {
        let queue_id = queue_id.as_u64() as usize;
        let page = queue_id / PAGE_SIZE;
        let offset = queue_id % PAGE_SIZE;

        if self.local.len() <= page {
            let Ok(senders) = self.senders.read() else {
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

pub(super) struct SenderPages<T: 'static> {
    pub(super) pages: Vec<Arc<[Sender<T>]>>,
    pub(super) epoch: VarInt,
}

impl<T: 'static> SenderPages<T> {
    #[inline]
    pub(super) fn new(epoch: VarInt) -> Self {
        Self {
            pages: Vec::with_capacity(8),
            epoch,
        }
    }
}

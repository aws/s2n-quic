// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::descriptor::Descriptor;
use crate::sync::mpsc;
use s2n_quic_core::varint::VarInt;
use std::sync::{Arc, RwLock};

pub struct Senders<T, const PAGE_SIZE: usize> {
    pub(super) senders: Arc<RwLock<SenderPages<T>>>,
    pub(super) local: Vec<Arc<[SenderEntry<T>]>>,
}

impl<T: 'static, const PAGE_SIZE: usize> Clone for Senders<T, PAGE_SIZE> {
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
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
        if !sender.descriptor.is_active() {
            return;
        }
        f(&sender.inner)
    }
}

pub struct Sender<T> {
    pub stream: mpsc::Sender<T>,
    pub control: mpsc::Sender<T>,
}

pub(super) struct SenderPages<T> {
    pub(super) pages: Vec<Arc<[SenderEntry<T>]>>,
    pub(super) epoch: VarInt,
}

impl<T> Default for SenderPages<T> {
    fn default() -> Self {
        Self {
            pages: Vec::with_capacity(8),
            epoch: VarInt::ZERO,
        }
    }
}

pub(super) struct SenderEntry<T> {
    pub(super) inner: Sender<T>,
    pub(super) descriptor: Descriptor<T>,
}

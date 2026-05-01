// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::precision,
    socket::{
        pool::descriptor,
        send::transmission::{Entry, Transmission},
    },
    sync::intrusive_queue,
};
use std::sync::atomic::{AtomicBool, Ordering};

pub trait Completion<Info, Meta>: Sized {
    type Completer: Completer<Info, Meta, Self>;

    fn upgrade(&self) -> Option<Self::Completer>;

    /// Check if the receiver is still alive without upgrading.
    /// Returns true if the receiver is still listening for completions.
    fn is_alive(&self) -> bool;
}

impl<Info, Meta> Completion<Info, Meta> for () {
    type Completer = ();

    fn upgrade(&self) -> Option<Self::Completer> {
        Some(())
    }

    fn is_alive(&self) -> bool {
        true
    }
}

pub trait Completer<Info, Meta, Completion> {
    fn complete(self, transmission: Entry<Info, Meta, Completion>);
}

impl<Info, Meta, Completion> Completer<Info, Meta, Completion> for () {
    fn complete(self, _transmission: Entry<Info, Meta, Completion>) {}
}

pub struct CompleteTransmission<'a, Info, Meta> {
    pub info: Info,
    pub segment: descriptor::Filled,
    pub transmission_time: precision::Timestamp,
    pub meta: &'a Meta,
}

pub struct Queue<Info, Meta, Completion> {
    completion: intrusive_queue::Queue<Transmission<Info, Meta, Completion>>,
    is_open: AtomicBool,
}

impl<Info, Meta, Completion> Default for Queue<Info, Meta, Completion> {
    #[inline]
    fn default() -> Self {
        Self {
            completion: intrusive_queue::Queue::new(),
            is_open: true.into(),
        }
    }
}

impl<Info, Meta, Completion> Queue<Info, Meta, Completion>
where
    Meta: Default,
{
    #[inline]
    pub fn alloc_entry(
        &self,
        completion_queue: impl FnOnce() -> Completion,
    ) -> Entry<Info, Meta, Completion> {
        let transmission = Transmission {
            descriptors: Default::default(),
            total_len: 0,
            meta: Default::default(),
            transmission_time: None,
            completion: completion_queue(),
        };
        Entry::new(transmission)
    }

    pub fn close(&self) {
        self.is_open.store(false, Ordering::Relaxed);
    }

    #[inline]
    pub fn complete_transmission(&self, entry: Entry<Info, Meta, Completion>) {
        if self.is_open.load(Ordering::Relaxed) {
            self.completion.push_back(entry);
        }
    }

    #[inline]
    pub fn drain_completion_queue(
        &self,
        mut on_transmission: impl FnMut(CompleteTransmission<Info, Meta>),
    ) -> usize {
        let mut count = 0;

        // try multiple times to drain the completion queue in case something was pushed while we were processing it
        loop {
            let mut remaining = self.completion.take();

            let mut did_process_transmission = false;
            while let Some(mut transmission) = remaining.pop_front() {
                {
                    let transmission = &mut *transmission;
                    let descriptors = core::mem::take(&mut transmission.descriptors);
                    let transmission_time = transmission.transmission_time.unwrap();
                    let meta = &transmission.meta;
                    for (segment, info) in descriptors.into_iter().map(|d| d.into_inner()) {
                        on_transmission(CompleteTransmission {
                            info,
                            segment,
                            transmission_time,
                            meta,
                        });
                    }
                }
                count += 1;
                did_process_transmission = true;
            }

            if !did_process_transmission {
                break;
            }
        }

        count
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::sync::{
    channel::{new as channel, Receiver, Sender},
    ring_deque::RingDeque,
};
use crossbeam_epoch::{pin, Atomic, Owned};
use s2n_quic_core::varint::VarInt;
use std::{
    mem::MaybeUninit,
    sync::{atomic::Ordering, Arc, Mutex},
};

pub struct Router<P> {
    senders: Arc<Senders<P>>,
}

impl<P> Clone for Router<P> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
        }
    }
}

impl<P> Default for Router<P> {
    fn default() -> Self {
        Self {
            senders: Default::default(),
        }
    }
}

impl<P> Router<P> {
    #[inline]
    pub fn send(&self, id: VarInt, packet: P) {
        let Ok(idx): Result<usize, _> = id.try_into() else {
            return;
        };

        self.senders.send(idx, packet);
    }
}

pub struct Allocator<P> {
    senders: Arc<Senders<P>>,
    deque: RingDeque<VarInt>,
}

struct Senders<P> {
    senders: Atomic<[MaybeUninit<Sender<P>>]>,
    channels: Mutex<Vec<(Sender<P>, Receiver<P>)>>,
    channel_cap: usize,
    max_len: usize,
}

impl<P> Default for Senders<P> {
    #[inline]
    fn default() -> Self {
        let len = 256;
        let channel_cap = 4096;
        let channels = (0..len).map(|_| channel(channel_cap)).collect::<Vec<_>>();
        let max_len = u32::MAX as usize;

        let mut senders_owned = Owned::<[MaybeUninit<Sender<P>>]>::init(len);

        for ((sender, _receiver), target) in channels.iter().zip(senders_owned.iter_mut()) {
            target.write(sender.clone());
        }

        let senders = Atomic::null();
        senders.store(senders_owned, Ordering::SeqCst);

        let channels = Mutex::new(channels);

        Self {
            senders,
            channels,
            channel_cap,
            max_len,
        }
    }
}

impl<P> Senders<P> {
    #[inline]
    fn send(&self, idx: usize, packet: P) {
        let pin = pin();
        let senders = self.senders.load(Ordering::Acquire, &pin);

        let Some(senders) = (unsafe { senders.as_ref() }) else {
            return;
        };

        let Some(sender) = senders.get(idx) else {
            return;
        };

        unsafe {
            let _ = sender.assume_init_ref().send_back(packet);
        }
    }

    #[inline]
    fn grow(&self) -> bool {
        let mut channels = self.channels.lock().unwrap();
        let len = channels.len() * 2;
        if len > self.max_len {
            return false;
        }
        channels.resize_with(len, || channel(self.channel_cap));

        let mut senders = Owned::<[MaybeUninit<Sender<P>>]>::init(len);

        for ((sender, _receiver), target) in channels.iter().zip(senders.iter_mut()) {
            target.write(sender.clone());
        }

        let pin = pin();
        let prev = self.senders.swap(senders, Ordering::Release, &pin);

        // clean up the previous version
        unsafe {
            drop(prev.into_owned());
        }

        true
    }
}

impl<P> Drop for Senders<P> {
    #[inline]
    fn drop(&mut self) {
        let ptr = core::mem::replace(&mut self.senders, Atomic::null());

        let Some(mut senders) = (unsafe { ptr.try_into_owned() }) else {
            return;
        };

        for sender in senders.iter_mut() {
            unsafe {
                sender.assume_init_drop();
            }
        }
    }
}

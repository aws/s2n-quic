// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Structure for concurrently queueing network messages
//!
//! Two halves are created: `Producer` and `Consumer`. The producer's role is:
//!
//! * Acquire capacity to send messages
//! * Fill the messages with some data
//! * Release the filled messages to the consumer
//!
//! The consumer then:
//!
//! * Acquires filled messages
//! * Reads the messages
//! * Releases the read messages back to the producer to be filled with more messages
//!
//! Normally, ring buffers wrap around the data region and return 2 slices of data (see
//! [`std::collections::VecDeque::as_mut_slices`]). This causes issues for syscalls like
//! [`libc::sendmmsg`] where it expects a single contiguous region of memory to be passed
//! for the messages. This would result in either having to make 2 syscalls for
//! when both slices have items (one of the more expensive operations we do) or copying all of the
//! messages out of the ring buffer into a [`Vec`] and passing that to the syscall. Neither of
//! these are ideal. Instead, the ring buffer capacity is doubled and split into a primary and
//! secondary region. The messages are replicated by the producer between the regions to ensure a
//! single slice can be taken at any arbitrary index by the consumer.
//!
//! Looking at an example, let's assume we have created a ring with capacity of 4. The ring will
//! actually allocate a memory region of 8 messages, where the first 4 point to the same payload
//! buffer as the last 4:
//!
//! ```ignore
//! [ primary   ]
//!              [ secondary ]
//! [ 0, 1, 2, 3, 0, 1, 2, 3 ]
//! ```
//!
//! We call the first half of the messages the "primary" region and the second half "secondary".
//! Now, let's assume that the current index of the producer is `2`. The region of memory returned
//! in the [`Producer::data`] call would be:
//!
//! ```ignore
//! [ primary   ]
//!              [ secondary ]
//! [ 0, 1, 2, 3, 0, 1, 2, 3 ]
//!        [ data     ]
//! ```
//!
//! If the producer fills the `data` slice with messages it will have written into both the primary
//! and secondary regions and those values need to be copied to the areas that weren't written to
//! in order to maintain a consistent view of the data:
//!
//! ```ignore
//! [ primary   ]
//!              [ secondary ]
//! [ 0, 1, 2, 3, 0, 1, 2, 3 ]
//!        [ data     ]
//!        [src ]  ->  [ dst ]
//! [ dst ]  <-  [src ]
//! ```
//!
//! When the consumer goes to read the queue it can do so at its own index without having to split
//! the data, even if it wraps around the end.

use crate::message::{self, Message};
use alloc::sync::Arc;
use core::{
    mem::size_of,
    ptr::NonNull,
    sync::atomic::AtomicU32,
    task::{Context, Poll},
};
use s2n_quic_core::{
    assume,
    sync::{
        atomic_waker,
        cursor::{self, AbsoluteIndex, Cursor},
        CachePadded,
    },
};

const CURSOR_SIZE: usize = size_of::<CachePadded<AtomicU32>>();
const PRODUCER_OFFSET: usize = 0;
const CONSUMER_OFFSET: usize = CURSOR_SIZE;
const DATA_OFFSET: usize = CURSOR_SIZE * 2;

mod probes {
    pub mod consumer {
        s2n_quic_core::extern_probe!(
            extern "probe" {
                /// Emitted when a consumer tries to acquire messages
                #[link_name = s2n_quic_platform__socket__ring__consumer__acquire]
                pub fn acquire(channel: *const (), index: u32, count: u32, capacity: u32);

                /// Emitted when a consumer releases finished messages
                #[link_name = s2n_quic_platform__socket__ring__consumer__release]
                pub fn release(channel: *const (), index: u32, count: u32, capacity: u32);
            }
        );
    }

    pub mod producer {
        s2n_quic_core::extern_probe!(
            extern "probe" {
                /// Emitted when a producer tries to acquire capacity to send messages
                #[link_name = s2n_quic_platform__socket__ring__producer__acquire]
                pub fn acquire(channel: *const (), index: u32, count: u32, capacity: u32);

                /// Emitted when a producer releases ready-to-be-read messages
                #[link_name = s2n_quic_platform__socket__ring__producer__release]
                pub fn release(channel: *const (), index: u32, count: u32, capacity: u32);
            }
        );
    }
}

/// Creates a pair of rings for a given message type
pub fn pair<T: Message>(entries: u32, payload_len: u32) -> (Producer<T>, Consumer<T>) {
    let storage = T::alloc(entries, payload_len, DATA_OFFSET);

    let storage = Arc::new(storage);
    let ptr = storage.as_ptr();

    let wakers = atomic_waker::pair();

    let consumer = Consumer {
        cursor: unsafe { builder(ptr, entries).build_consumer() },
        wakers: wakers.0,
        storage: storage.clone(),
    };

    let producer = Producer {
        cursor: unsafe { builder(ptr, entries).build_producer() },
        wakers: wakers.1,
        storage,
    };

    (producer, consumer)
}

/// A consumer ring for messages
pub struct Consumer<T: Message> {
    cursor: Cursor<T>,
    wakers: atomic_waker::Handle,
    #[allow(dead_code)]
    storage: Arc<message::Storage>,
}

/// Safety: Storage is synchronized with the Cursor
unsafe impl<T: Message> Send for Consumer<T> {}
/// Safety: Storage is synchronized with the Cursor
unsafe impl<T: Message> Sync for Consumer<T> {}

impl<T: Message> Consumer<T> {
    /// Acquires ready-to-consume messages from the producer
    #[inline]
    pub fn acquire(&mut self, watermark: u32) -> u32 {
        let amount = self.cursor.acquire_consumer(watermark);

        probes::consumer::acquire(
            self.as_ptr(),
            self.cursor.cached_consumer(),
            amount,
            self.cursor.capacity(),
        );

        amount
    }

    /// Polls ready-to-consume messages from the producer
    #[inline]
    pub fn poll_acquire(&mut self, watermark: u32, cx: &mut Context) -> Poll<u32> {
        macro_rules! try_acquire {
            () => {{
                let count = self.acquire(watermark);

                if count > 0 {
                    return Poll::Ready(count);
                }
            }};
        }

        // first try to acquire some messages
        try_acquire!();

        // if we couldn't acquire anything register our waker
        self.wakers.register(cx.waker());

        // try to acquire some messages in case we got some concurrently to waker registration
        try_acquire!();

        Poll::Pending
    }

    /// Releases consumed messages to the producer
    #[inline]
    pub fn release(&mut self, release_len: u32) {
        if release_len == 0 {
            return;
        }

        probes::consumer::release(
            self.as_ptr(),
            self.cursor.cached_consumer(),
            release_len,
            self.cursor.capacity(),
        );

        self.cursor.release_consumer(release_len);
        self.wakers.wake();
    }

    /// Returns the currently acquired messages
    #[inline]
    pub fn data(&mut self) -> &mut [T] {
        let idx = self.cursor.cached_consumer();
        let len = self.cursor.cached_consumer_len();
        let ptr = self.cursor.data_ptr();
        unsafe {
            let ptr = ptr.as_ptr().add(idx as _);
            core::slice::from_raw_parts_mut(ptr, len as _)
        }
    }

    /// Returns true if the producer is not closed
    #[inline]
    pub fn is_open(&self) -> bool {
        self.wakers.is_open()
    }

    #[inline]
    pub(crate) fn absolute_index(&self) -> AbsoluteIndex {
        self.cursor.consumer_abs_index()
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const () {
        self.storage.as_ptr() as *const ()
    }
}

/// A producer ring for messages
pub struct Producer<T: Message> {
    cursor: Cursor<T>,
    wakers: atomic_waker::Handle,
    #[allow(dead_code)]
    storage: Arc<message::Storage>,
}

/// Safety: Storage is synchronized with the Cursor
unsafe impl<T: Message> Send for Producer<T> {}
/// Safety: Storage is synchronized with the Cursor
unsafe impl<T: Message> Sync for Producer<T> {}

impl<T: Message> Producer<T> {
    /// Acquires capacity for sending messages to the consumer
    #[inline]
    pub fn acquire(&mut self, watermark: u32) -> u32 {
        let amount = self.cursor.acquire_producer(watermark);

        probes::producer::acquire(
            self.as_ptr(),
            self.cursor.cached_producer(),
            amount,
            self.cursor.capacity(),
        );

        amount
    }

    /// Polls capacity for sending messages to the consumer
    #[inline]
    pub fn poll_acquire(&mut self, watermark: u32, cx: &mut Context) -> Poll<u32> {
        macro_rules! try_acquire {
            () => {{
                let count = self.acquire(watermark);

                if count > 0 {
                    return Poll::Ready(count);
                }
            }};
        }

        // first try to acquire some messages
        try_acquire!();

        // if we couldn't acquire anything register our waker
        self.wakers.register(cx.waker());

        // try to acquire some messages in case we got some concurrently to waker registration
        try_acquire!();

        Poll::Pending
    }

    /// Releases ready-to-consume messages to the consumer
    #[inline]
    pub fn release(&mut self, release_len: u32) {
        if release_len == 0 {
            return;
        }

        debug_assert!(
            release_len <= self.cursor.cached_producer_len(),
            "cannot release more messages than acquired"
        );

        let idx = self.cursor.cached_producer();
        let ring_size = self.cursor.capacity();

        probes::producer::release(self.as_ptr(), idx, release_len, ring_size);

        // replicate any written items to the secondary region
        unsafe {
            assume!(ring_size > idx, "idx should never exceed the ring size");

            // calculate the maximum number of replications we need to perform for the primary ->
            // secondary
            let max_possible_replications = ring_size - idx;
            // the replication count should exceed the number that we're releasing
            let replication_count = max_possible_replications.min(release_len);

            assume!(
                replication_count != 0,
                "we should always be releasing at least 1 item"
            );

            // calculate the data pointer based on the current message index
            let primary = self.cursor.data_ptr().as_ptr().add(idx as _);
            // add the size of the ring to the primary pointer to get into the secondary message
            let secondary = primary.add(ring_size as _);

            // copy the primary into the secondary
            self.replicate(primary, secondary, replication_count as _);

            // if messages were also written to the secondary region, we need to copy them back to the
            // primary region
            assume!(
                idx.checked_add(release_len).is_some(),
                "overflow amount should not exceed u32::MAX"
            );
            assume!(
                idx + release_len < ring_size * 2,
                "overflow amount should not extend beyond the secondary replica"
            );

            let overflow_amount = (idx + release_len).checked_sub(ring_size).filter(|v| {
                // we didn't overflow if the count is 0
                *v > 0
            });

            if let Some(replication_count) = overflow_amount {
                // secondary -> primary replication always happens at the beginning of the data
                let primary = self.cursor.data_ptr().as_ptr();
                // add the size of the ring to the primary pointer to get into the secondary
                // message
                let secondary = primary.add(ring_size as _);

                // copy the secondary into the primary
                self.replicate(secondary, primary, replication_count as _);
            }
        }

        // finally release the len to the consumer
        self.cursor.release_producer(release_len);

        // wake up the consumer to notify it of progress
        self.wakers.wake();
    }

    /// Returns the empty messages for the producer
    #[inline]
    pub fn data(&mut self) -> &mut [T] {
        let idx = self.cursor.cached_producer();
        let len = self.cursor.cached_producer_len();
        let ptr = self.cursor.data_ptr();
        unsafe {
            let ptr = ptr.as_ptr().add(idx as _);
            core::slice::from_raw_parts_mut(ptr, len as _)
        }
    }

    /// Returns true if the consumer is not closed
    #[inline]
    pub fn is_open(&self) -> bool {
        self.wakers.is_open()
    }

    /// Replicates messages from the primary to secondary memory regions
    #[inline]
    unsafe fn replicate(&self, primary: *mut T, secondary: *mut T, len: usize) {
        debug_assert_ne!(len, 0);

        #[cfg(debug_assertions)]
        {
            let primary = core::slice::from_raw_parts(primary, len as _);
            let secondary = core::slice::from_raw_parts(secondary, len as _);
            for (primary, secondary) in primary.iter().zip(secondary) {
                T::validate_replication(primary, secondary);
            }
        }

        core::ptr::copy_nonoverlapping(primary, secondary, len as _);
    }

    #[inline]
    pub(crate) fn absolute_index(&self) -> AbsoluteIndex {
        self.cursor.producer_abs_index()
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const () {
        self.storage.as_ptr() as *const ()
    }
}

#[inline]
unsafe fn builder<T: Message>(ptr: *mut u8, size: u32) -> cursor::Builder<T> {
    let producer = ptr.add(PRODUCER_OFFSET) as *mut _;
    let producer = NonNull::new(producer).unwrap();
    let consumer = ptr.add(CONSUMER_OFFSET) as *mut _;
    let consumer = NonNull::new(consumer).unwrap();
    let data = ptr.add(DATA_OFFSET) as *mut _;
    let data = NonNull::new(data).unwrap();

    cursor::Builder {
        producer,
        consumer,
        data,
        size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use s2n_quic_core::{
        inet::{ExplicitCongestionNotification, SocketAddress},
        path::{Handle as _, LocalAddress, RemoteAddress},
    };

    #[cfg(not(kani))]
    type Counts = Vec<u32>;
    #[cfg(kani)]
    type Counts = s2n_quic_core::testing::InlineVec<u32, 2>;

    macro_rules! replication_test {
        ($name:ident, $msg:ty) => {
            #[test]
            #[cfg_attr(kani, kani::proof, kani::solver(cadical), kani::unwind(3))]
            #[cfg(any(not(kani), kani_slow))] // this test takes too much memory for our CI
                                              // environment
            fn $name() {
                check!().with_type::<Counts>().for_each(|counts| {
                    let entries = if cfg!(kani) { 2 } else { 16 };
                    let payload_len = if cfg!(kani) { 2 } else { 128 };

                    let (mut producer, mut consumer) = pair::<$msg>(entries, payload_len);

                    let mut counter = 0;

                    for count in counts.iter().copied() {
                        let count = producer.acquire(count);

                        for entry in &mut producer.data()[..count as usize] {
                            unsafe {
                                entry.set_payload_len(counter);
                            }
                            counter += 1;
                        }

                        producer.release(count);

                        #[cfg(kani)]
                        let ids_to_check = {
                            let idx: u32 = kani::any();
                            kani::assume(idx < entries);
                            idx..idx + 1
                        };

                        #[cfg(not(kani))]
                        let ids_to_check = 0..entries;

                        for idx in ids_to_check {
                            let ptr = producer.cursor.data_ptr().as_ptr();
                            unsafe {
                                let primary = &*ptr.add(idx as _);
                                let secondary = &*ptr.add((idx + entries) as _);

                                assert_eq!(primary.payload_len(), secondary.payload_len());
                            }
                        }

                        let count = consumer.acquire(count);
                        consumer.release(count);
                    }
                });
            }
        };
    }

    replication_test!(simple_replication, crate::message::simple::Message);
    #[cfg(s2n_quic_platform_socket_msg)]
    replication_test!(msg_replication, crate::message::msg::Message);
    #[cfg(s2n_quic_platform_socket_mmsg)]
    replication_test!(mmsg_replication, crate::message::mmsg::Message);

    macro_rules! send_recv_test {
        ($name:ident, $msg:ty) => {
            #[test]
            fn $name() {
                check!().with_type::<Counts>().for_each(|counts| {
                    let entries = if cfg!(miri) { 2 } else { 16 };
                    let payload_len = if cfg!(miri) { 4 } else { 128 };

                    let (mut producer, mut consumer) = pair::<$msg>(entries, payload_len);

                    let mut tx_counter = 0u32;
                    let mut rx_counter = 0u32;

                    let local_address = LocalAddress::from(SocketAddress::default());

                    for count in counts.iter().copied() {
                        let count = producer.acquire(count);

                        for entry in &mut producer.data()[..count as usize] {
                            unsafe {
                                entry.reset(payload_len as _);
                            }

                            let mut remote_address = SocketAddress::default();
                            remote_address.set_port(tx_counter as _);
                            let remote_address = RemoteAddress::from(remote_address);
                            let handle =
                                <$msg as Message>::Handle::from_remote_address(remote_address);
                            let ecn = ExplicitCongestionNotification::new(tx_counter as _);
                            let payload = tx_counter.to_le_bytes();
                            let msg = (handle, ecn, &payload[..]);
                            entry.tx_write(msg).unwrap();
                            tx_counter += 1;
                        }

                        producer.release(count);

                        let count = consumer.acquire(count);
                        for entry in consumer.data() {
                            let message = entry.rx_read(&local_address).unwrap();
                            message.for_each(|header, payload| {
                                if <$msg>::SUPPORTS_ECN {
                                    let ecn = ExplicitCongestionNotification::new(rx_counter as _);
                                    assert_eq!(header.ecn, ecn);
                                }

                                let counter: &[u8; 4] = (&*payload).try_into().unwrap();
                                let counter = u32::from_le_bytes(*counter);
                                assert_eq!(counter, rx_counter);

                                rx_counter += 1;
                            });
                        }
                        consumer.release(count);
                    }
                });
            }
        };
    }

    send_recv_test!(simple_send_recv, crate::message::simple::Message);
    #[cfg(s2n_quic_platform_socket_msg)]
    send_recv_test!(msg_send_recv, crate::message::msg::Message);
    #[cfg(s2n_quic_platform_socket_mmsg)]
    send_recv_test!(mmsg_send_recv, crate::message::mmsg::Message);
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{MmapOffsets, RingFlags, RingOffsetV2, RxTxDescriptor, UmemDescriptor},
    mmap::Mmap,
    socket, syscall,
};
use core::{fmt, mem::size_of, ptr::NonNull};
use s2n_quic_core::sync::cursor::{self, Cursor};
use std::{io, os::unix::io::AsRawFd};

#[derive(Debug)]
#[allow(dead_code)] // we hold on to `area` and `socket` to ensure they live long enough
struct Ring<T: Copy + fmt::Debug> {
    cursor: Cursor<T>,
    flags: NonNull<RingFlags>,
    // make the area clonable in test mode
    #[cfg(test)]
    area: std::sync::Arc<Mmap>,
    #[cfg(not(test))]
    area: Mmap,
    socket: socket::Fd,
}

impl<T: Copy + fmt::Debug> Ring<T> {
    /// Returns `true` if the ring needs to be notified when entries are updated
    ///
    /// See [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L87).
    #[inline]
    pub fn needs_wakeup(&self) -> bool {
        self.flags().contains(RingFlags::NEED_WAKEUP)
    }

    /// Returns a reference to the flags on the ring
    #[inline]
    pub fn flags(&self) -> &RingFlags {
        unsafe { &*self.flags.as_ptr() }
    }

    /// Returns a mutable reference to the flags on the ring
    #[inline]
    #[cfg(test)]
    pub fn flags_mut(&mut self) -> &mut RingFlags {
        unsafe { &mut *self.flags.as_ptr() }
    }
}

/// Safety: the Mmap area is held for as long as the Cursor
unsafe impl<T: Copy + fmt::Debug> Send for Ring<T> {}

/// Safety: the Mmap area is held for as long as the Cursor
unsafe impl<T: Copy + fmt::Debug> Sync for Ring<T> {}

#[inline]
unsafe fn builder<T: Copy>(
    area: &Mmap,
    offsets: &RingOffsetV2,
    size: u32,
) -> (cursor::Builder<T>, NonNull<RingFlags>) {
    let producer = area.addr().as_ptr().add(offsets.producer as _);
    let producer = NonNull::new_unchecked(producer as _);
    let consumer = area.addr().as_ptr().add(offsets.consumer as _);
    let consumer = NonNull::new_unchecked(consumer as _);

    let flags = area.addr().as_ptr().add(offsets.flags as _);
    let flags = NonNull::new_unchecked(flags as *mut RingFlags);

    let descriptors = area.addr().as_ptr().add(offsets.desc as _);
    let descriptors = NonNull::new_unchecked(descriptors as *mut T);

    let builder = cursor::Builder {
        producer,
        consumer,
        data: descriptors,
        size,
    };

    (builder, flags)
}

macro_rules! impl_producer {
    ($T:ty, $syscall:ident, $field:ident, $offset:ident) => {
        /// Creates a new ring with the given configuration
        pub fn new(socket: socket::Fd, offsets: &MmapOffsets, size: u32) -> io::Result<Self> {
            syscall::$syscall(&socket, size)?;
            let offsets = &offsets.$field;

            // start with the descriptor offset as the total length
            let mut len = offsets.desc as usize;
            // extend the length by the `size` multiplied the entry size
            len += size as usize * size_of::<$T>();

            // Use the hard-coded offset of the ring type
            let offset = MmapOffsets::$offset;

            let area = Mmap::new(len, offset, Some(socket.as_raw_fd()))?;

            let (cursor, flags) = unsafe {
                // Safety: `area` lives as long as `cursor`
                let (builder, flags) = builder(&area, offsets, size);
                (builder.build_producer(), flags)
            };

            #[cfg(test)]
            let area = std::sync::Arc::new(area);

            Ok(Self(Ring {
                cursor,
                flags,
                area,
                socket,
            }))
        }

        /// Acquire a number of entries for the ring
        ///
        /// If the cached length is lower than the provided `watermark` no synchronization will be
        /// performed.
        #[inline]
        pub fn acquire(&mut self, watermark: u32) -> u32 {
            self.0.cursor.acquire_producer(watermark)
        }

        /// Releases `len` number of entries to the consumer side of the ring.
        ///
        /// # Panics
        ///
        /// If the `len` exceeds the number of acquired entries, this function will panic.
        #[inline]
        pub fn release(&mut self, len: u32) {
            self.0.cursor.release_producer(len)
        }

        /// Returns `true` if the ring needs to be woken up in order to notify the kernel
        #[inline]
        pub fn needs_wakeup(&self) -> bool {
            self.0.needs_wakeup()
        }

        /// Returns the unfilled entries for the producer
        ///
        /// After filling `n` entries, the `release` function should be called with `n`.
        #[inline]
        pub fn data(&mut self) -> (&mut [$T], &mut [$T]) {
            unsafe {
                // Safety: this is the producer for the ring
                self.0.cursor.producer_data()
            }
        }

        /// Returns the overall size of the ring
        #[inline]
        pub fn capacity(&self) -> usize {
            self.0.cursor.capacity() as _
        }

        #[inline]
        pub fn len(&self) -> u32 {
            self.0.cursor.cached_producer_len()
        }

        #[inline]
        pub fn is_empty(&self) -> bool {
            self.0.cursor.cached_producer_len() == 0
        }

        #[inline]
        pub fn is_full(&self) -> bool {
            self.0.cursor.cached_producer_len() == self.0.cursor.capacity()
        }

        /// Returns the socket associated with the ring
        #[inline]
        pub fn socket(&self) -> &socket::Fd {
            &self.0.socket
        }
    };
}

macro_rules! impl_consumer {
    ($T:ty, $syscall:ident, $field:ident, $offset:ident) => {
        /// Creates a new ring with the given configuration
        pub fn new(socket: socket::Fd, offsets: &MmapOffsets, size: u32) -> io::Result<Self> {
            syscall::$syscall(&socket, size)?;
            let offsets = &offsets.$field;

            // start with the descriptor offset as the total length
            let mut len = offsets.desc as usize;
            // extend the length by the `size` multiplied the entry size
            len += size as usize * size_of::<$T>();

            // Use the hard-coded offset of the ring type
            let offset = MmapOffsets::$offset;

            let area = Mmap::new(len, offset, Some(socket.as_raw_fd()))?;

            let (cursor, flags) = unsafe {
                // Safety: `area` lives as long as `cursor`
                let (builder, flags) = builder(&area, offsets, size);
                (builder.build_consumer(), flags)
            };

            #[cfg(test)]
            let area = std::sync::Arc::new(area);

            Ok(Self(Ring {
                cursor,
                flags,
                area,
                socket,
            }))
        }

        /// Acquire a number of entries for the ring
        ///
        /// If the cached length is lower than the provided `watermark` no synchronization will be
        /// performed.
        #[inline]
        pub fn acquire(&mut self, watermark: u32) -> u32 {
            self.0.cursor.acquire_consumer(watermark)
        }

        /// Releases `len` number of entries to the producer side of the ring.
        ///
        /// # Panics
        ///
        /// If the `len` exceeds the number of acquired entries, this function will panic.
        #[inline]
        pub fn release(&mut self, len: u32) {
            self.0.cursor.release_consumer(len)
        }

        /// Returns the filled entries for the consumer
        ///
        /// After filling `n` entries, the `release` function should be called with `n`.
        #[inline]
        pub fn data(&mut self) -> (&mut [$T], &mut [$T]) {
            unsafe {
                // Safety: this is the consumer for the ring
                self.0.cursor.consumer_data()
            }
        }

        /// Returns the overall size of the ring
        #[inline]
        pub fn capacity(&self) -> usize {
            self.0.cursor.capacity() as _
        }

        #[inline]
        pub fn len(&self) -> u32 {
            self.0.cursor.cached_consumer_len()
        }

        #[inline]
        pub fn is_empty(&self) -> bool {
            self.0.cursor.cached_consumer_len() == 0
        }

        #[inline]
        pub fn is_full(&self) -> bool {
            self.0.cursor.cached_consumer_len() == self.0.cursor.capacity()
        }

        /// Returns the socket associated with the ring
        #[inline]
        pub fn socket(&self) -> &socket::Fd {
            &self.0.socket
        }

        #[cfg(test)]
        pub fn set_flags(&mut self, flags: crate::if_xdp::RingFlags) {
            *self.0.flags_mut() = flags;
        }
    };
}

/// A transmission ring for entries to be transmitted
#[derive(Debug)]
pub struct Tx(Ring<RxTxDescriptor>);

impl Tx {
    impl_producer!(RxTxDescriptor, set_tx_ring_size, tx, TX_RING);
}

/// A receive ring for entries to be processed
#[derive(Debug)]
pub struct Rx(Ring<RxTxDescriptor>);

impl Rx {
    impl_consumer!(RxTxDescriptor, set_rx_ring_size, rx, RX_RING);
}

/// The fill ring for entries to be populated
#[derive(Debug)]
pub struct Fill(Ring<UmemDescriptor>);

impl Fill {
    impl_producer!(UmemDescriptor, set_fill_ring_size, fill, FILL_RING);

    /// Initializes the ring with the given Umem descriptors
    ///
    /// # Panics
    ///
    /// This should only be called at initialization and will panic if called on a non-full ring.
    #[inline]
    pub fn init<I: Iterator<Item = UmemDescriptor>>(&mut self, descriptors: I) {
        assert!(self.is_full());

        let (head, tail) = self.data();
        let items = head.iter_mut().chain(tail);
        let mut count = 0;
        for (item, desc) in items.zip(descriptors) {
            *item = desc;
            count += 1;
        }
        self.release(count);
    }
}

/// The completion ring for entries to be reused for transmission
#[derive(Debug)]
pub struct Completion(Ring<UmemDescriptor>);

impl Completion {
    impl_consumer!(
        UmemDescriptor,
        set_completion_ring_size,
        completion,
        COMPLETION_RING
    );

    /// Initializes the ring with the given Umem descriptors
    ///
    /// # Panics
    ///
    /// This should only be called at initialization and will panic if called on a non-empty ring.
    #[inline]
    pub fn init<I: Iterator<Item = UmemDescriptor>>(&mut self, descriptors: I) {
        assert!(self.is_empty());

        {
            // pretend we're the producer so we can push items to ourselves
            let size = self.capacity() as u32;
            self.0
                .cursor
                .producer()
                .fetch_add(size, core::sync::atomic::Ordering::SeqCst);

            self.acquire(size);
        }

        let (head, tail) = self.data();
        let items = head.iter_mut().chain(tail);
        for (item, desc) in items.zip(descriptors) {
            *item = desc;
        }
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::{if_xdp, socket::Fd};

    fn offsets() -> if_xdp::RingOffsetV2 {
        if_xdp::RingOffsetV2 {
            producer: 0,
            consumer: core::mem::size_of::<usize>() as _,
            flags: (core::mem::size_of::<usize>() * 2) as _,
            desc: (core::mem::size_of::<usize>() * 3) as _,
        }
    }

    macro_rules! impl_pair {
        ($name:ident, $consumer:ident, $producer:ident, $T:ident) => {
            /// Creates a pair of rings used for testing
            pub fn $name(size: u32) -> ($consumer, $producer) {
                assert!(size.is_power_of_two());

                let offsets = offsets();

                // start with the descriptor offset as the total length
                let mut len = offsets.desc as usize;
                // extend the length by the `size` multiplied the entry size
                len += size as usize * size_of::<$T>();

                let area = Mmap::new(len, 0, None).unwrap();

                let (consumer_cursor, flags) = unsafe {
                    // Safety: `area` lives as long as `cursor`
                    let (builder, flags) = builder(&area, &offsets, size);
                    (builder.build_consumer(), flags)
                };

                let producer_cursor = unsafe {
                    // Safety: `area` lives as long as `cursor`
                    let (builder, _flags) = builder(&area, &offsets, size);
                    builder.build_producer()
                };

                let area = std::sync::Arc::new(area);

                let cons = $consumer(Ring {
                    cursor: consumer_cursor,
                    flags,
                    area: area.clone(),
                    socket: Fd::from_raw(-1),
                });

                let prod = $producer(Ring {
                    cursor: producer_cursor,
                    flags,
                    area,
                    socket: Fd::from_raw(-1),
                });

                (cons, prod)
            }
        };
    }

    impl_pair!(rx_tx, Rx, Tx, RxTxDescriptor);
    impl_pair!(completion_fill, Completion, Fill, UmemDescriptor);

    #[test]
    fn rx_tx_test() {
        let _ = rx_tx(16);
    }

    #[test]
    fn comp_fill_test() {
        let _ = completion_fill(16);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{MmapOffsets, RxTxDescriptor, UmemDescriptor},
    mmap::Mmap,
    socket, syscall,
};
use core::{fmt, mem::size_of};
use std::{io, os::fd::AsRawFd};

mod cursor;

use cursor::Cursor;

#[derive(Debug)]
struct Ring<T: Copy + fmt::Debug> {
    cursor: Cursor<T>,
    area: Mmap,
    socket: socket::Fd,
}

/// Safety: the Mmap area is held for as long as the Cursor
unsafe impl<T: Copy + fmt::Debug> Send for Ring<T> {}

/// Safety: the Mmap area is held for as long as the Cursor
unsafe impl<T: Copy + fmt::Debug> Sync for Ring<T> {}

macro_rules! impl_producer {
    ($T:ty, $syscall:ident, $field:ident, $offset:ident) => {
        /// Creates a new ring with the given configuration
        pub fn new(socket: socket::Fd, offsets: &MmapOffsets, size: u32) -> io::Result<Self> {
            syscall::$syscall(&socket, size)?;
            let offsets = &offsets.$field;

            let len = offsets.desc as usize + size as usize * size_of::<$T>();
            let offset = MmapOffsets::$offset;

            let area = Mmap::new(len, offset, Some(socket.as_raw_fd()))?;

            let mut cursor = unsafe { Cursor::new(&area, offsets, size) };

            // initialize the cached producer cursor
            cursor.acquire_producer(u32::MAX);

            Ok(Self(Ring {
                cursor,
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
            self.0.cursor.needs_wakeup()
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
    };
}

macro_rules! impl_consumer {
    ($T:ty, $syscall:ident, $field:ident, $offset:ident) => {
        /// Creates a new ring with the given configuration
        pub fn new(socket: socket::Fd, offsets: &MmapOffsets, size: u32) -> io::Result<Self> {
            syscall::$syscall(&socket, size)?;
            let offsets = &offsets.$field;

            let len = offsets.desc as usize + size as usize * size_of::<$T>();
            let offset = MmapOffsets::$offset;

            let area = Mmap::new(len, offset, Some(socket.as_raw_fd()))?;

            let cursor = unsafe { Cursor::new(&area, offsets, size) };

            Ok(Self(Ring {
                cursor,
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
    };
}

/// A transmission ring for entries to be transmitted
pub struct Tx(Ring<RxTxDescriptor>);

impl Tx {
    impl_producer!(RxTxDescriptor, set_tx_ring_size, tx, TX_RING);
}

/// A receive ring for entries to be processed
pub struct Rx(Ring<RxTxDescriptor>);

impl Rx {
    impl_consumer!(RxTxDescriptor, set_rx_ring_size, rx, RX_RING);
}

/// The fill ring for entries to be populated
pub struct Fill(Ring<UmemDescriptor>);

impl Fill {
    impl_producer!(UmemDescriptor, set_fill_ring_size, fill, FILL_RING);
}

/// The completion ring for entries to be reused for transmission
pub struct Completion(Ring<UmemDescriptor>);

impl Completion {
    impl_consumer!(
        UmemDescriptor,
        set_completion_ring_size,
        completion,
        COMPLETION_RING
    );
}

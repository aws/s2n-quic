// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{MmapOffsets, RxTxDescriptor, UmemDescriptor},
    mmap::Mmap,
    socket, syscall,
};
use core::{fmt, mem::size_of};
use std::{io, os::unix::io::AsRawFd};

mod cursor;

use cursor::Cursor;

#[derive(Debug)]
#[allow(dead_code)] // we hold on to `area` and `socket` to ensure they live long enough
struct Ring<T: Copy + fmt::Debug> {
    cursor: Cursor<T>,
    // make the area clonable in test mode
    #[cfg(test)]
    area: std::sync::Arc<Mmap>,
    #[cfg(not(test))]
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

            // start with the descriptor offset as the total length
            let mut len = offsets.desc as usize;
            // extend the length by the `size` multiplied the entry size
            len += size as usize * size_of::<$T>();

            // Use the hard-coded offset of the ring type
            let offset = MmapOffsets::$offset;

            let area = Mmap::new(len, offset, Some(socket.as_raw_fd()))?;

            let mut cursor = unsafe {
                // Safety: `area` lives as long as `cursor`
                Cursor::new(&area, offsets, size)
            };

            unsafe {
                // Safety: this is only called by a producer
                cursor.init_producer();
            }

            #[cfg(test)]
            let area = std::sync::Arc::new(area);

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

        /// Returns the overall size of the ring
        #[inline]
        pub fn capacity(&self) -> usize {
            self.0.cursor.capacity() as _
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

            let cursor = unsafe {
                // Safety: `area` lives as long as `cursor`
                Cursor::new(&area, offsets, size)
            };

            #[cfg(test)]
            let area = std::sync::Arc::new(area);

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

        /// Returns the overall size of the ring
        #[inline]
        pub fn capacity(&self) -> usize {
            self.0.cursor.capacity() as _
        }

        #[cfg(test)]
        pub fn set_flags(&mut self, flags: crate::if_xdp::RingFlags) {
            *self.0.cursor.flags_mut() = flags;
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

                let consumer_cursor = unsafe {
                    // Safety: `area` lives as long as `cursor`
                    Cursor::new(&area, &offsets, size)
                };

                let mut producer_cursor = unsafe {
                    // Safety: `area` lives as long as `cursor`
                    Cursor::new(&area, &offsets, size)
                };

                unsafe {
                    // Safety: this is only called by a producer
                    producer_cursor.init_producer();
                }

                let area = std::sync::Arc::new(area);

                let cons = $consumer(Ring {
                    cursor: consumer_cursor,
                    area: area.clone(),
                    socket: Fd::invalid(),
                });

                let prod = $producer(Ring {
                    cursor: producer_cursor,
                    area,
                    socket: Fd::invalid(),
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

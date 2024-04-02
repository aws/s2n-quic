// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::{Bytes, BytesMut};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::path::MINIMUM_MAX_DATAGRAM_SIZE;

/// Allocates a large single buffer, rather than several small buffers
///
/// Used for sending packets that contain CONNECTION_CLOSE frames.
#[derive(Debug)]
pub struct Buffer {
    buffer: BytesMut,
    max_size: usize,
    count: usize,
}

// This number shouldn't be _too_ small, otherwise we're performing a bunch
// of allocations. It also shouldn't be _too_ big so we hold on to those allocations
// for an extended period of time.
const DEFAULT_PACKETS: usize = 64;

impl Default for Buffer {
    fn default() -> Self {
        Self {
            buffer: BytesMut::new(),
            max_size: MINIMUM_MAX_DATAGRAM_SIZE as usize,
            count: DEFAULT_PACKETS,
        }
    }
}

impl Buffer {
    pub fn write<F: FnOnce(EncoderBuffer) -> EncoderBuffer>(
        &mut self,
        on_write: F,
    ) -> Option<Bytes> {
        let max_size = self.max_size;

        if self.buffer.capacity() < max_size {
            let len = max_size * self.count;
            let mut buffer = BytesMut::with_capacity(len);
            // extend the length of the buffer to the capacity so we can
            // take a slice of it
            //
            // We could use `bytes::UninitSlice` but EncoderBuffer uses a
            // concrete slice instead.
            unsafe {
                // Safety: the EncoderBuffer only allows writing (no reading) from
                //         uninitialized memory
                buffer.set_len(len);
            }
            self.buffer = buffer;
        }

        let buffer = EncoderBuffer::new(&mut self.buffer[..max_size]);

        let new_buff = on_write(buffer);

        let len = max_size - new_buff.remaining_capacity();

        if len == 0 {
            return None;
        }

        debug_assert!(len <= max_size);

        Some(self.buffer.split_to(len).freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_test() {
        let mut buffer = Buffer::default();
        assert_eq!(buffer.buffer.capacity(), 0);

        assert!(
            buffer.write(|buffer| buffer).is_none(),
            "empty writes should return None"
        );

        assert!(buffer.buffer.capacity() > 0);
    }

    #[test]
    fn non_empty_test() {
        let mut buffer = Buffer::default();

        let packet = buffer
            .write(|mut buffer| {
                assert_eq!(
                    buffer.remaining_capacity(),
                    MINIMUM_MAX_DATAGRAM_SIZE as usize,
                    "the provider buffer should be the MINIMUM_MAX_DATAGRAM_SIZE"
                );
                buffer.encode(&1337u16);
                buffer
            })
            .expect("non-empty writes should return a packet");

        assert_eq!(packet, 1337u16.to_be_bytes()[..]);

        assert!(buffer.buffer.capacity() > 0);
        assert!(
            buffer.buffer.capacity() < MINIMUM_MAX_DATAGRAM_SIZE as usize * DEFAULT_PACKETS,
            "space should be trimmed off for the returned packet"
        );
    }
}

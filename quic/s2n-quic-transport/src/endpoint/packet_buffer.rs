// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::{Bytes, BytesMut};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::path::MINIMUM_MTU;

/// Allocates a large single buffer, rather than several small buffers
///
/// Used for sending packets that contain CONNECTION_CLOSE frames.
#[derive(Debug)]
pub struct Buffer {
    buffer: BytesMut,
    max_size: usize,
    count: usize,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            buffer: BytesMut::new(),
            max_size: MINIMUM_MTU as usize,
            count: 64,
        }
    }
}

impl Buffer {
    pub fn write<F: FnOnce(EncoderBuffer) -> Result<EncoderBuffer, E>, E>(
        &mut self,
        on_write: F,
    ) -> Result<Bytes, E> {
        let max_size = self.max_size;

        if self.buffer.capacity() < max_size {
            let len = max_size * self.count;
            let mut buffer = BytesMut::with_capacity(len);
            unsafe {
                // Safety: the encoder doesn't read any data from this region
                buffer.set_len(len);
            }
            self.buffer = buffer;
        }

        let buffer = EncoderBuffer::new(&mut self.buffer[..max_size]);

        let new_buff = on_write(buffer)?;

        let len = max_size - new_buff.len();

        debug_assert!(len <= max_size);

        Ok(self.buffer.split_to(len).freeze())
    }
}

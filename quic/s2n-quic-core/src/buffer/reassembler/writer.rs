// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Reassembler, Slot};
use crate::{
    buffer::{reader::Storage as _, Error, Reader, Writer},
    varint::VarInt,
};
use bytes::BytesMut;

impl Writer for Reassembler {
    #[inline]
    fn read_from<R>(&mut self, reader: &mut R) -> Result<(), Error<R::Error>>
    where
        R: Reader + ?Sized,
    {
        // enable checks for the reader
        let mut reader = reader.with_checks();
        let reader = &mut reader;

        let final_offset = reader.final_offset();

        // optimize for the case where the stream consists of a single chunk
        if let Some(final_offset) = final_offset {
            let mut is_single_chunk_stream = true;

            let offset = reader.current_offset();

            // the reader is starting at the beginning of the stream
            is_single_chunk_stream &= offset == VarInt::ZERO;
            // the reader has buffered the final offset
            is_single_chunk_stream &= reader.has_buffered_fin();
            // no data has been consumed from the Reassembler
            is_single_chunk_stream &= self.consumed_len() == 0;
            // we aren't tracking any slots
            is_single_chunk_stream &= self.slots.is_empty();

            if is_single_chunk_stream {
                let payload_len = reader.buffered_len();
                let end = final_offset.as_u64();

                // don't allocate anything if we don't need to
                if payload_len == 0 {
                    let chunk = reader.read_chunk(0)?;
                    debug_assert!(chunk.is_empty());
                } else {
                    let mut data = BytesMut::with_capacity(payload_len);

                    // copy the whole thing into `data`
                    reader.copy_into(&mut data)?;

                    self.slots.push_back(Slot::new(offset.as_u64(), end, data));
                };

                // update the final offset after everything was read correctly
                self.final_offset = end;
                self.invariants();

                return Ok(());
            }
        }

        // TODO add better support for copy avoidance by iterating to the appropriate slot and
        // copying into that, if possible

        // fall back to copying individual chunks into the receive buffer
        let mut first_write = true;
        loop {
            let offset = reader.current_offset();
            let chunk = reader.read_chunk(usize::MAX)?;

            // Record the final size before writing to avoid excess allocation. This also needs to
            // happen after we read the first chunk in case there are errors.
            if first_write {
                if let Some(offset) = final_offset {
                    self.write_at_fin(offset, &[]).map_err(Error::mapped)?;
                }
            }

            // TODO maybe specialize on BytesMut chunks? - for now we'll just treat them as
            // slices

            self.write_at(offset, &chunk).map_err(Error::mapped)?;

            first_write = false;

            if reader.buffer_is_empty() {
                break;
            }
        }

        Ok(())
    }
}

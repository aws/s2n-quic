// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic_core::buffer::{reader, writer, Reader};

pub struct Packet<'a, 'p, D: decrypt::Key, C: Clock> {
    pub packet: &'a mut stream::decoder::Packet<'p>,
    pub payload_cursor: usize,
    pub is_decrypted_in_place: bool,
    pub ecn: ExplicitCongestionNotification,
    pub clock: &'a C,
    pub opener: &'a D,
    pub receiver: &'a mut Receiver,
}

impl<'a, 'p, D: decrypt::Key, C: Clock> reader::Storage for Packet<'a, 'p, D, C> {
    type Error = Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.packet.payload().len() - self.payload_cursor
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<reader::storage::Chunk<'_>, Self::Error> {
        if !self.is_decrypted_in_place {
            self.receiver.on_stream_packet_in_place(
                self.opener,
                self.packet,
                self.ecn,
                self.clock,
            )?;
            self.is_decrypted_in_place = true;
        }

        let payload = &self.packet.payload()[self.payload_cursor..];
        let len = payload.len().min(watermark);
        self.payload_cursor += len;
        let payload = &payload[..len];
        Ok(payload.into())
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<reader::storage::Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut should_read_chunk = false;

        // we've already decrypted the payload
        should_read_chunk |= self.is_decrypted_in_place;

        // we've already started reading the payload
        should_read_chunk |= self.payload_cursor > 0;

        // we don't actually have a payload to decrypt so we should cache the authentication
        should_read_chunk |= self.packet.payload().is_empty();

        // the destination is smaller than the payload length
        should_read_chunk |= self.packet.payload().len() > dest.remaining_capacity();

        if should_read_chunk {
            return self.read_chunk(dest.remaining_capacity());
        }

        let did_write = dest.put_uninit_slice(self.packet.payload().len(), |dest| {
            self.receiver.on_stream_packet_copy(
                self.opener,
                self.packet,
                self.ecn,
                dest,
                self.clock,
            )
        })?;

        // if the destination wasn't capable of handling the uninit slice, then fall back
        if !did_write {
            return self.read_chunk(dest.remaining_capacity());
        }

        self.payload_cursor = self.packet.payload().len();

        Ok(Default::default())
    }
}

impl<'a, 'p, D: decrypt::Key, C: Clock> Reader for Packet<'a, 'p, D, C> {
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.packet.stream_offset() + self.payload_cursor
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.packet.final_offset()
    }
}

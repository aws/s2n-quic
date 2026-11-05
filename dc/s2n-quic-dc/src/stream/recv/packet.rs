// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto, event,
    packet::stream,
    stream::recv::{state::State as Receiver, Error},
};
use s2n_quic_core::{
    buffer::{reader, writer, Reader},
    ensure,
    inet::ExplicitCongestionNotification,
    time::Clock,
    varint::VarInt,
};

pub struct Packet<'a, 'p, D, K, C, Pub>
where
    D: crypto::open::Application,
    K: crypto::open::control::Stream,
    C: Clock + ?Sized,
    Pub: event::ConnectionPublisher,
{
    pub packet: &'a mut stream::decoder::Packet<'p>,
    pub payload_cursor: usize,
    pub decryption: Decryption,
    pub ecn: ExplicitCongestionNotification,
    pub clock: &'a C,
    pub opener: &'a D,
    pub control: &'a K,
    pub receiver: &'a mut Receiver,
    pub copied_len: usize,
    pub publisher: &'a Pub,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decryption {
    Pending,
    InPlace,
    /// Plaintext is in the destination; the packet payload remains ciphertext.
    Copied,
}

impl<D, K, C: Clock + ?Sized, Pub> Packet<'_, '_, D, K, C, Pub>
where
    D: crypto::open::Application,
    K: crypto::open::control::Stream,
    Pub: event::ConnectionPublisher,
{
    /// Opens unread ciphertext without consuming payload bytes.
    #[inline]
    pub fn authenticate(&mut self) -> Result<(), Error> {
        if self.decryption != Decryption::Pending {
            return Ok(());
        }

        self.packet.decrypt_in_place(self.opener, self.control)?;
        self.decryption = Decryption::InPlace;
        self.receiver
            .on_cleartext_stream_packet(self.packet, self.ecn, self.clock, self.publisher)
    }
}

impl<D, K, C: Clock, Pub> reader::Storage for Packet<'_, '_, D, K, C, Pub>
where
    D: crypto::open::Application,
    K: crypto::open::control::Stream,
    C: Clock + ?Sized,
    Pub: event::ConnectionPublisher,
{
    type Error = Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.packet.payload().len() - self.payload_cursor
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<reader::storage::Chunk<'_>, Self::Error> {
        self.authenticate()?;
        if self.decryption == Decryption::Copied {
            return Ok(Default::default());
        }

        let payload = &self.packet.payload()[self.payload_cursor..];
        let len = payload.len().min(watermark);
        self.payload_cursor += len;
        self.copied_len += len;
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
        should_read_chunk |= self.decryption != Decryption::Pending;

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
            self.packet.decrypt(self.opener, self.control, dest)?;
            self.decryption = Decryption::Copied;
            self.receiver.on_cleartext_stream_packet(
                self.packet,
                self.ecn,
                self.clock,
                self.publisher,
            )
        })?;

        // if the destination wasn't capable of handling the uninit slice, then fall back
        if !did_write {
            return self.read_chunk(dest.remaining_capacity());
        }

        let len = self.packet.payload().len();
        self.payload_cursor = len;
        self.copied_len += len;

        Ok(Default::default())
    }
}

impl<D, K, C: Clock, Pub> Reader for Packet<'_, '_, D, K, C, Pub>
where
    D: crypto::open::Application,
    K: crypto::open::control::Stream,
    C: Clock + ?Sized,
    Pub: event::ConnectionPublisher,
{
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.packet.stream_offset() + self.payload_cursor
    }

    #[inline]
    fn skip_until(&mut self, offset: VarInt) -> Result<(), Self::Error> {
        ensure!(offset > self.current_offset(), Ok(()));

        // Reassembly may skip buffered data without reading any payload. Authenticate before
        // advancing so a forged FIN is rejected while the reassembler can roll back its cursors.
        self.authenticate()?;

        let remaining = offset - self.current_offset();
        let remaining = remaining.as_u64() as usize;
        self.payload_cursor = (self.payload_cursor + remaining).min(self.packet.payload().len());

        // don't count skipped bytes as copied

        Ok(())
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.packet.final_offset()
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::{decrypt, UninitSlice},
    packet::datagram::{decoder, Tag},
};
use s2n_codec::{decoder_invariant, DecoderBufferMut, DecoderError};
use s2n_quic_core::packet::number::{PacketNumberSpace, SlidingWindow, SlidingWindowError};

pub use crate::crypto::decrypt::Error;
pub use decoder::Packet;

#[derive(Default)]
pub struct Endpoint {}

impl Endpoint {
    pub fn parse<'a>(&self, payload: &'a mut [u8]) -> Option<(Packet<'a>, &'a mut [u8])> {
        let buffer = DecoderBufferMut::new(payload);
        let (packet, buffer) = Packet::decode(buffer, TagValidator, 16).ok()?;
        let buffer = buffer.into_less_safe_slice();
        Some((packet, buffer))
    }
}

struct TagValidator;

impl decoder::Validator for TagValidator {
    #[inline]
    fn validate_tag(&mut self, tag: Tag) -> Result<(), DecoderError> {
        decoder_invariant!(!tag.ack_eliciting(), "expected tunnelled datagram");
        decoder_invariant!(
            !tag.has_application_header(),
            "application headers currently unsupported"
        );
        Ok(())
    }
}

pub struct Receiver<K: decrypt::Key> {
    key: K,
}

impl<K: decrypt::Key> Receiver<K> {
    pub fn new(key: K) -> Self {
        Self { key }
    }

    pub fn recv_into(
        &mut self,
        packet: &Packet,
        payload_out: &mut UninitSlice,
    ) -> Result<(), Error> {
        debug_assert_eq!(packet.payload().len(), payload_out.len());

        self.key.decrypt(
            packet.crypto_nonce(),
            packet.header(),
            packet.payload(),
            packet.auth_tag(),
            payload_out,
        )?;

        Ok(())
    }
}

#[derive(Default)]
pub struct SeenFilter {
    window: SlidingWindow,
}

impl SeenFilter {
    #[inline]
    pub fn on_packet(&mut self, packet: &Packet) -> Result<(), SlidingWindowError> {
        let packet_number =
            PacketNumberSpace::ApplicationData.new_packet_number(packet.packet_number());
        self.window.insert(packet_number)
    }
}

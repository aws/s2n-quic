// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::DecoderBufferMut;
use s2n_quic_core::varint::VarInt;

pub type PacketNumber = VarInt;
pub type HeaderLen = VarInt;
pub type PayloadLen = VarInt;

#[macro_use]
pub mod tag;

pub mod control;
pub mod datagram;
pub mod secret_control;
pub mod stream;

pub use tag::Tag;

#[derive(Debug)]
pub enum Packet<'a> {
    Stream(stream::decoder::Packet<'a>),
    Datagram(datagram::decoder::Packet<'a>),
    Control(control::decoder::Packet<'a>),
    StaleKey(secret_control::stale_key::Packet<'a>),
    ReplayDetected(secret_control::replay_detected::Packet<'a>),
    RequestShards(secret_control::request_shards::Packet<'a>),
    UnknownPathSecret(secret_control::unknown_path_secret::Packet<'a>),
}

impl<'a> s2n_codec::DecoderParameterizedValueMut<'a> for Packet<'a> {
    type Parameter = usize;

    #[inline]
    fn decode_parameterized_mut(
        tag_len: Self::Parameter,
        decoder: DecoderBufferMut<'a>,
    ) -> s2n_codec::DecoderBufferMutResult<Self> {
        match decoder.peek().decode().map(|(tag, _)| tag)? {
            Tag::Control(_) => {
                let (packet, decoder) = control::decoder::Packet::decode(decoder, (), tag_len)?;
                Ok((Self::Control(packet), decoder))
            }
            Tag::Stream(_) => {
                let (packet, decoder) = stream::decoder::Packet::decode(decoder, (), tag_len)?;
                Ok((Self::Stream(packet), decoder))
            }
            Tag::Datagram(_) => {
                let (packet, decoder) = datagram::decoder::Packet::decode(decoder, (), tag_len)?;
                Ok((Self::Datagram(packet), decoder))
            }
            Tag::StaleKey(_) => {
                // TODO
                todo!()
            }
            Tag::ReplayDetected(_) => {
                // TODO
                todo!()
            }
            Tag::RequestShards(_) => {
                // TODO
                todo!()
            }
            Tag::UnknownPathSecret(_) => {
                // TODO
                todo!()
            }
        }
    }
}

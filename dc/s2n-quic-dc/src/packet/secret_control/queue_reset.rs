// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::credentials::Credentials;

impl_tag!(QUEUE_RESET);
impl_packet!(QueueReset, {
    #[inline]
    pub const fn queue_id(&self) -> VarInt {
        self.value.queue_id
    }

    #[inline]
    pub const fn tag(&self) -> Tag {
        Tag(Tag::VALUE)
    }

    #[inline]
    pub const fn credentials(&self) -> &Credentials {
        &self.value.credentials
    }

    #[inline]
    pub fn credential_id(&self) -> &crate::credentials::Id {
        &self.value.credentials.id
    }

    #[inline]
    pub const fn trigger(&self) -> Trigger {
        self.value.trigger
    }
});

/// Indicates which packet type triggered the QueueReset.
///
/// When an unroutable packet is received, a QueueReset is generated and sent back
/// to the originator. The `Trigger` field tells the receiver which half of the
/// stream caused the reset, so it can be routed to the correct worker:
///
/// - `Stream` → the sender's stream data was rejected → route to control queue (send worker)
/// - `Control` → the receiver's ACKs were rejected → route to stream queue (recv worker)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub enum Trigger {
    /// The QueueReset was triggered by an unroutable stream packet.
    ///
    /// This means the send worker's data was rejected, so the QueueReset should
    /// be routed to the control queue (send worker).
    #[default]
    Stream = 0,
    /// The QueueReset was triggered by an unroutable control packet.
    ///
    /// This means the recv worker's ACKs were rejected, so the QueueReset should
    /// be routed to the stream queue (recv worker).
    Control = 1,
}

impl Trigger {
    /// Returns `true` if the QueueReset was triggered by a stream packet
    #[inline]
    pub const fn is_stream(&self) -> bool {
        matches!(self, Self::Stream)
    }

    /// Returns `true` if the QueueReset was triggered by a control packet
    #[inline]
    pub const fn is_control(&self) -> bool {
        matches!(self, Self::Control)
    }
}

impl EncoderValue for Trigger {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        VarInt::from_u8(*self as u8).encode(encoder)
    }
}

impl<'a> DecoderValue<'a> for Trigger {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (value, buffer) = buffer.decode::<VarInt>()?;
        let trigger = match value.as_u64() {
            0 => Self::Stream,
            1 => Self::Control,
            // Default to Stream for forward compatibility
            _ => Self::Stream,
        };
        Ok((trigger, buffer))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct QueueReset {
    pub credentials: Credentials,
    pub wire_version: WireVersion,
    pub queue_id: VarInt,
    pub code: VarInt,
    pub trigger: Trigger,
}

impl QueueReset {
    #[inline]
    pub fn encode<C>(&self, mut encoder: EncoderBuffer, crypto: &C) -> usize
    where
        C: seal::control::Secret,
    {
        encoder.encode(&Tag::default());
        encoder.encode(&self.credentials);
        encoder.encode(&self.wire_version);
        encoder.encode(&self.queue_id);
        encoder.encode(&self.code);
        encoder.encode(&self.trigger);

        encoder::finish(encoder, crypto)
    }
}

impl<'a> DecoderValue<'a> for QueueReset {
    #[inline]
    fn decode(buffer: DecoderBuffer<'a>) -> R<'a, Self> {
        let (_tag, buffer) = buffer.decode::<Tag>()?;
        let (credentials, buffer) = buffer.decode()?;
        let (wire_version, buffer) = buffer.decode()?;
        let (queue_id, buffer) = buffer.decode()?;
        let (code, buffer) = buffer.decode()?;
        let (trigger, buffer) = buffer.decode()?;
        let value = Self {
            wire_version,
            credentials,
            queue_id,
            code,
            trigger,
        };
        Ok((value, buffer))
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Behavior, Segment};
use crate::message;
use core::ops::{Deref, DerefMut};
use s2n_quic_core::{
    inet::datagram,
    io::{rx, tx},
    path::{self, LocalAddress},
};

/// A view of the currently enqueued messages for a given segment
#[derive(Debug)]
pub struct Slice<'a, Message: message::Message, Behavior> {
    /// A slice of all of the messages in the buffer
    pub(crate) messages: &'a mut [Message],
    /// Reference to the primary segment
    pub(crate) primary: &'a mut Segment,
    /// Reference to the secondary segment
    pub(crate) secondary: &'a mut Segment,
    /// Reset the messages after use
    pub(crate) behavior: Behavior,
    /// The maximum allowed number of GSO segments
    pub(crate) max_gso: usize,
    /// The index to the previously pushed segment
    pub(crate) gso_segment: Option<GsoSegment>,
    /// The base handle for all of the messages to inherit
    pub(crate) local_address: &'a LocalAddress,
}

#[derive(Debug, Default)]
pub struct GsoSegment {
    index: usize,
    count: usize,
    size: usize,
}

impl<'a, Message: message::Message, B: Behavior> Slice<'a, Message, B> {
    /// Finishes the borrow of the `Slice` with a specified `count`
    ///
    /// Calling this method will move `count` messages from one segment
    /// to the other; e.g. `ready` to `pending`.
    #[inline]
    pub fn finish(mut self, count: usize) {
        self.advance(count);
    }

    /// Advances the primary slice by the specified `count`
    fn advance(&mut self, count: usize) {
        debug_assert!(
            count <= self.len(),
            "cannot finish more messages than available"
        );

        self.flush_gso();

        let (start, end, overflow, capacity) = self.compute_behavior_arguments(count);

        let (primary, secondary) = self.messages.split_at_mut(capacity);

        self.behavior
            .advance(primary, secondary, start, end, overflow);
        self.primary.move_into(self.secondary, count);
    }

    /// Preserves the messages in the current segment
    pub fn cancel(mut self, count: usize) {
        self.flush_gso();

        let (start, end, overflow, capacity) = self.compute_behavior_arguments(count);

        let (primary, secondary) = self.messages.split_at_mut(capacity);

        self.behavior
            .cancel(primary, secondary, start, end, overflow);
    }

    #[inline]
    fn compute_behavior_arguments(&self, count: usize) -> (usize, usize, usize, usize) {
        let capacity = self.primary.capacity;
        let prev_index = self.primary.index;
        let new_index = prev_index + count;

        let start = prev_index;
        let end = new_index.min(capacity);
        let overflow = new_index.saturating_sub(capacity);

        (start, end, overflow, capacity)
    }
}

impl<'a, Message: message::Message, B> Slice<'a, Message, B> {
    /// Flushes the current GSO message, if any
    ///
    /// In the `gso_segment` field, we track which message is currently being
    /// built. If there ended up being multiple payloads written to the single message
    /// we need to set the msg_control values to indicate the GSO size.
    #[inline]
    fn flush_gso(&mut self) {
        if !Message::SUPPORTS_GSO {
            return;
        }

        if let Some(gso) = self.gso_segment.take() {
            // only set the `msg_control` if there was more than one payload written to the message
            if gso.count > 1 {
                // since messages are double the number of payloads, we need to calculate a primary
                // and secondary index so we can accurately replicate the fields.
                let mid = self.messages.len() / 2;
                let (primary, secondary) = self.messages.split_at_mut(mid);
                let index = gso.index;

                // try to wrap around the midpoint
                let (primary, secondary) = if let Some(index) = index.checked_sub(mid) {
                    let primary = &mut primary[index];
                    let secondary = &mut secondary[index];
                    (secondary, primary)
                } else {
                    let primary = &mut primary[index];
                    let secondary = &mut secondary[index];
                    (primary, secondary)
                };

                // let the primary message know that we sent multiple payloads in a single message
                primary.set_segment_size(gso.size);
                // replicate the fields from the primary to the secondary
                secondary.replicate_fields_from(primary);
            }
        }
    }

    /// Tries to send a message as a GSO segment
    ///
    /// Returns the Err(Message) if it was not able to. Otherwise, the index of the GSO'd message is returned.
    #[inline]
    fn try_gso<M: tx::Message<Handle = Message::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<Result<tx::Outcome, M>, tx::Error> {
        if !Message::SUPPORTS_GSO {
            return Ok(Err(message));
        }

        let gso = if let Some(gso) = self.gso_segment.as_mut() {
            gso
        } else {
            return Ok(Err(message));
        };

        let max_segments = self.max_gso;
        debug_assert!(
            max_segments > 1,
            "gso_segment should only be set when max_gso > 1"
        );

        let prev_message = &mut self.messages[gso.index];
        // check to make sure the message can be GSO'd and can be included in the same
        // GSO payload as the previous message
        if !(message.can_gso(gso.size, gso.count) && prev_message.can_gso(&mut message)) {
            self.flush_gso();
            return Ok(Err(message));
        }

        debug_assert!(
            gso.count < max_segments,
            "{} cannot exceed {}",
            gso.count,
            max_segments
        );

        let payload_len = prev_message.payload_len();

        unsafe {
            // Safety: all payloads should have enough capacity to extend max_segments *
            // gso.size
            prev_message.set_payload_len(payload_len + gso.size);
        }

        // allow the message to write up to `gso.size` bytes
        let buffer = &mut message::Message::payload_mut(prev_message)[payload_len..];
        let buffer = tx::PayloadBuffer::new(buffer);

        match message.write_payload(buffer, gso.count).and_then(|size| {
            // we don't want to send empty packets
            if size == 0 {
                Err(tx::Error::EmptyPayload)
            } else {
                Ok(size)
            }
        }) {
            Err(err) => {
                unsafe {
                    // revert the len to what it was before
                    prev_message.set_payload_len(payload_len);
                }
                Err(err)
            }
            Ok(size) => {
                debug_assert_ne!(size, 0, "payloads should never be empty");

                unsafe {
                    debug_assert!(
                        gso.size >= size,
                        "the payload tried to write more than available"
                    );
                    // set the len to the actual amount written to the payload
                    prev_message.set_payload_len(payload_len + size.min(gso.size));
                }
                // increment the number of segments that we've written
                gso.count += 1;

                debug_assert!(
                    gso.count <= max_segments,
                    "{} cannot exceed {}",
                    gso.count,
                    max_segments
                );

                let index = gso.index;

                // the last segment can be smaller but we can't write any more if it is
                let size_mismatch = gso.size != size;

                // we're bounded by the max_segments amount
                let at_segment_limit = gso.count >= max_segments;

                // we also can't write more data than u16::MAX
                let at_payload_limit = gso.size * (gso.count + 1) > u16::MAX as usize;

                // if we've hit any limits, then flush the GSO information to the message
                if size_mismatch || at_segment_limit || at_payload_limit {
                    self.flush_gso();
                }

                Ok(Ok(tx::Outcome { len: size, index }))
            }
        }
    }
}

impl<'a, Message: message::Message, R> Drop for Slice<'a, Message, R> {
    #[inline]
    fn drop(&mut self) {
        self.flush_gso()
    }
}

impl<'a, Message: message::Message, R> Deref for Slice<'a, Message, R> {
    type Target = [Message];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.messages[self.primary.range()]
    }
}

impl<'a, Message: message::Message, R> DerefMut for Slice<'a, Message, R> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.messages[self.primary.range()]
    }
}

impl<'a, Message: message::Message<Handle = H>, B: Behavior, H: path::Handle> rx::Queue
    for Slice<'a, Message, B>
{
    type Handle = H;

    #[inline]
    fn for_each<F: FnMut(datagram::Header<H>, &mut [u8])>(&mut self, mut on_packet: F) {
        // get the currently filled packets
        let range = self.primary.range();

        let len = range.len();

        // iterate over the filled packets and invoke the callback for each one
        let messages = &mut self.messages[range];
        for message in messages {
            if let Some((header, payload)) = message.rx_read(self.local_address) {
                on_packet(header, payload);
            }
        }

        // consume all of the messages
        self.advance(len);
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.primary.len == 0
    }
}

impl<'a, Message: message::Message<Handle = H>, B: Behavior, H: path::Handle> tx::Queue
    for Slice<'a, Message, B>
{
    type Handle = H;

    #[inline]
    fn push<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        message: M,
    ) -> Result<tx::Outcome, tx::Error> {
        // first try to write a GSO payload
        let message = match self.try_gso(message)? {
            Ok(outcome) => return Ok(outcome),
            Err(message) => message,
        };

        // find the index of the current message
        let index = self
            .primary
            .index(self.secondary)
            .ok_or(tx::Error::AtCapacity)?;

        let size = self.messages[index].tx_write(message)?;
        self.advance(1);

        // if we support GSO then mark the message as GSO-capable
        if Message::SUPPORTS_GSO && self.max_gso > 1 {
            self.gso_segment = Some(GsoSegment {
                index,
                count: 1,
                size,
            });
        }

        Ok(tx::Outcome { len: size, index })
    }

    #[inline]
    #[allow(unknown_lints, clippy::misnamed_getters)] // this slice is made up of two halves and uses the primary for unfilled data
    fn capacity(&self) -> usize {
        self.primary.len
    }
}

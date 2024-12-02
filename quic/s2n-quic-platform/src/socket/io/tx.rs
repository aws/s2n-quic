// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{features::Gso, message::Message, socket::ring::Producer};
use core::task::{Context, Poll};
use s2n_quic_core::{
    event,
    inet::ExplicitCongestionNotification,
    io::tx,
    path::{Handle as _, MaxMtu},
    task::waker,
};

/// Structure for sending messages to producer channels
pub struct Tx<T: Message> {
    channels: Vec<Producer<T>>,
    gso: Gso,
    max_mtu: usize,
    is_full: bool,
}

impl<T: Message> Tx<T> {
    #[inline]
    pub fn new(channels: Vec<Producer<T>>, gso: Gso, max_mtu: MaxMtu) -> Self {
        Self {
            channels,
            gso,
            max_mtu: max_mtu.into(),
            is_full: true,
        }
    }
}

impl<T: Message> tx::Tx for Tx<T> {
    type PathHandle = T::Handle;
    type Queue = TxQueue<'static, T>;
    type Error = ();

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // We only need to poll for capacity if we completely filled up all of the channels.
        // If we always polled, this would cause the endpoint to spin since most of the time it has
        // capacity for sending.
        if !self.is_full {
            return Poll::Pending;
        }

        // NOTE: we don't wrap the above check in the contract as we'd technically violate the
        // contract since we're returning `Pending` without storing a waker
        waker::debug_assert_contract(cx, |cx| {
            let mut is_any_ready = false;
            let mut is_all_closed = true;

            for channel in &mut self.channels {
                match channel.poll_acquire(1, cx) {
                    Poll::Ready(_) => {
                        is_all_closed = false;
                        is_any_ready = true;
                    }
                    Poll::Pending => {
                        is_all_closed &= !channel.is_open();
                    }
                }
            }

            // if all of the channels were closed then shut the task down
            if is_all_closed {
                return Err(()).into();
            }

            // if any of the channels became ready then wake the endpoint up
            if is_any_ready {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
    }

    #[inline]
    fn queue<F: FnOnce(&mut Self::Queue)>(&mut self, f: F) {
        let this: &'static mut Self = unsafe {
            // Safety: As noted in the [transmute examples](https://doc.rust-lang.org/std/mem/fn.transmute.html#examples)
            // it can be used to temporarily extend the lifetime of a reference. In this case, we
            // don't want to use GATs until the MSRV is >=1.65.0, which means `Self::Queue` is not
            // allowed to take generic lifetimes.
            //
            // We are left with using a `'static` lifetime here and encapsulating it in a private
            // field. The `Self::Queue` struct is then borrowed for the lifetime of the `F`
            // function. This will prevent the value from escaping beyond the lifetime of `&mut
            // self`.
            //
            // See https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=9a32abe85c666f36fb2ec86496cc41b4
            //
            // Once https://github.com/aws/s2n-quic/issues/1742 is resolved this code can go away
            core::mem::transmute(self)
        };

        let mut capacity = 0;
        let mut first_with_free_slots = None;
        for (idx, channel) in this.channels.iter_mut().enumerate() {
            // try to make one more effort to acquire capacity for sending
            let count = channel.acquire(u32::MAX) as usize;

            if count > 0 && first_with_free_slots.is_none() {
                // find the first channel that had capacity
                first_with_free_slots = Some(idx);
            }

            capacity += count;
        }

        // mark that we're still full so we need to poll and wake up next iteration
        this.is_full = capacity == 0;

        // start with the first queue that has free slots, otherwise set the index to the length,
        // which will return an AtCapacity error immediately.
        let channel_index = first_with_free_slots.unwrap_or(this.channels.len());

        // query the maximum number of segments we can fill at this point in time
        //
        // NOTE: this value could be lowered in the case the TX task encounters an error with GSO
        //       so we do need to query it each iteration.
        let max_segments = this.gso.max_segments();

        let mut queue = TxQueue {
            channels: &mut this.channels,
            channel_index,
            message_index: 0,
            pending_release: 0,
            gso_segment: None,
            max_segments,
            max_mtu: this.max_mtu,
            capacity,
            is_full: &mut this.is_full,
        };

        f(&mut queue);
    }

    #[inline]
    fn handle_error<E: event::EndpointPublisher>(self, _error: Self::Error, _events: &mut E) {
        // The only reason we would be returning an error is if a channel closed. This could either
        // be because the endpoint is shutting down or one of the tasks panicked. Either way, we
        // don't know what the cause is here so we don't have any events to emit.
    }
}

/// Tracks the current state of a GSO message
#[derive(Debug, Default)]
pub struct GsoSegment<Handle> {
    /// The path handle of the current GSO segment being written
    ///
    /// This is used to determine if future messages should be included in this payload or need a
    /// separate packet.
    handle: Handle,
    /// The value of the ecn markings for the current GSO segment being written.
    ///
    /// This is used to determine if future messages should be included in this payload or need a
    /// separate packet.
    ecn: ExplicitCongestionNotification,
    /// The number of segments that have been written
    count: usize,
    /// The size of each segment.
    ///
    /// Note that the last segment can be smaller than the previous ones and will result in a flush
    size: usize,
}

pub struct TxQueue<'a, T: Message> {
    channels: &'a mut [Producer<T>],
    /// The channel index that we are currently operating on.
    ///
    /// This will be incremented after each channel is filled until it exceeds the len of `channels`.
    channel_index: usize,
    /// The message index into the current channel that we are operating on.
    ///
    /// This is incremented after each message is finished until it exceeds the acquired free
    /// slots, after which the `channel_index` is incremented (and message_index is reset to zero).
    message_index: usize,
    /// The number of messages in the current channel that need to be released to notify the
    /// consumer.
    ///
    /// This is to avoid calling `release` for each message and waking up the socket task too much.
    pending_release: u32,
    /// The current GSO segment we are filling, if any
    gso_segment: Option<GsoSegment<T::Handle>>,
    /// The maximum number of GSO segments that can be written
    max_segments: usize,
    /// The maximum MTU for any given packet
    max_mtu: usize,
    /// The maximum number of packets that can be sent in the current iteration
    capacity: usize,
    /// Used to track if we have filled up the producer queue and waiting on free slots to be
    /// released by the consumer.
    is_full: &'a mut bool,
}

impl<T: Message> TxQueue<'_, T> {
    /// Tries to send a message as a GSO segment
    ///
    /// Returns the Err(Message) if it was not able to. Otherwise, the index of the GSO'd message is returned.
    #[inline]
    fn try_gso<M: tx::Message<Handle = T::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<Result<tx::Outcome, M>, tx::Error> {
        // the message doesn't support GSO to return it
        if !T::SUPPORTS_GSO {
            return Ok(Err(message));
        }

        let max_segments = self.max_segments;

        let (prev_message, gso) = if let Some(gso) = self.gso_message() {
            gso
        } else {
            return Ok(Err(message));
        };

        debug_assert!(
            max_segments > 1,
            "gso_segment should only be set when max_gso > 1"
        );

        // check to make sure the message can be GSO'd and can be included in the same
        // GSO payload as the previous message
        let can_gso = message.can_gso(gso.size, gso.count)
            && message.path_handle().strict_eq(&gso.handle)
            && message.ecn() == gso.ecn;

        // if we can't use GSO then flush the current message
        if !can_gso {
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

        let buffer = unsafe {
            // Create a slice the `message` can write into. This avoids having to update the
            // payload length and worrying about panic safety.

            let payload = prev_message.payload_ptr_mut();

            // Safety: all payloads should have enough capacity to extend max_segments *
            // gso.size
            let current_payload = payload.add(payload_len);
            core::slice::from_raw_parts_mut(current_payload, gso.size)
        };
        let buffer = tx::PayloadBuffer::new(buffer);

        let size = message.write_payload(buffer, gso.count)?;

        // we don't want to send empty packets
        if size == 0 {
            return Err(tx::Error::EmptyPayload);
        }

        unsafe {
            debug_assert!(
                gso.size >= size,
                "the payload tried to write more than available"
            );
            // Set the len to the actual amount written to the payload. In case there is a bug,
            // take the min anyway so we don't have errors elsewhere.
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

        Ok(Ok(tx::Outcome {
            len: size,
            index: 0,
        }))
    }

    /// Flushes the current GSO message, if any
    ///
    /// In the `gso_segment` field, we track which message is currently being
    /// built. If there ended up being multiple payloads written to the single message
    /// we need to set the msg_control values to indicate the GSO size.
    #[inline]
    fn flush_gso(&mut self) {
        // no need to flush if the message type doesn't support GSO
        if !T::SUPPORTS_GSO {
            debug_assert!(
                self.gso_segment.is_none(),
                "gso_segment should not be set if GSO is unsupported"
            );
            return;
        }

        if let Some((message, gso)) = self.gso_message() {
            // only need to set the segment size if there was more than one payload written to the message
            if gso.count > 1 {
                message.set_segment_size(gso.size);
            }

            // clear out the current state and release the message
            self.gso_segment = None;
            self.release_message();
        }
    }

    /// Returns the current GSO message waiting for more segments
    #[inline]
    fn gso_message(&mut self) -> Option<(&mut T, &mut GsoSegment<T::Handle>)> {
        let gso = self.gso_segment.as_mut()?;

        let channel = unsafe {
            // Safety: the channel_index should always be in-bound if gso_segment is set
            s2n_quic_core::assume!(self.channels.len() > self.channel_index);
            &mut self.channels[self.channel_index]
        };

        let message = unsafe {
            // Safety: the message_index should always be in-bound if gso_segment is set
            let data = channel.data();
            s2n_quic_core::assume!(data.len() > self.message_index);
            &mut data[self.message_index]
        };

        Some((message, gso))
    }

    /// Releases the current message and marks it pending for release
    #[inline]
    fn release_message(&mut self) {
        self.capacity -= 1;
        *self.is_full = self.capacity == 0;

        let channel = unsafe {
            // Safety: the channel_index should always be in-bound if gso_segment is set
            s2n_quic_core::assume!(self.channels.len() > self.channel_index);
            &mut self.channels[self.channel_index]
        };

        channel.release_no_wake(1);

        self.pending_release += 1;
    }

    /// Flushes the current channel and releases any pending messages
    #[inline]
    fn flush_channel(&mut self) {
        if self.pending_release > 0 {
            if let Some(channel) = self.channels.get_mut(self.channel_index) {
                channel.wake();
                self.message_index = 0;
                self.pending_release = 0;
            }
        }
    }
}

impl<T: Message> tx::Queue for TxQueue<'_, T> {
    type Handle = T::Handle;

    const SUPPORTS_ECN: bool = T::SUPPORTS_ECN;
    const SUPPORTS_FLOW_LABELS: bool = T::SUPPORTS_FLOW_LABELS;

    #[inline]
    fn push<M>(&mut self, message: M) -> Result<tx::Outcome, tx::Error>
    where
        M: tx::Message<Handle = Self::Handle>,
    {
        // first try to write a GSO payload, if supported
        let mut message = match self.try_gso(message)? {
            Ok(outcome) => return Ok(outcome),
            Err(message) => message,
        };

        // find the next free entry, if any
        let entry = loop {
            let channel = self
                .channels
                .get_mut(self.channel_index)
                .ok_or(tx::Error::AtCapacity)?;

            if let Some(entry) = channel.data().get_mut(self.message_index) {
                break entry;
            } else {
                // this channel is out of free messages so flush it and move to the next channel
                self.flush_channel();
                self.channel_index += 1;
            };
        };

        // prepare the entry for writing and reset all of the fields
        unsafe {
            // Safety: the entries should have been allocated with the MaxMtu
            entry.reset(self.max_mtu);
        }

        // query the values that we use for GSO before we write the message to the entry
        let handle = *message.path_handle();
        let ecn = message.ecn();
        let can_gso = message.can_gso(self.max_mtu, 0);

        // write the message to the entry
        let payload_len = entry.tx_write(message)?;

        // if GSO is supported and we are allowed to have additional segments, store the GSO state
        // for another potential message to be written later
        if T::SUPPORTS_GSO && self.max_segments > 1 && can_gso {
            self.gso_segment = Some(GsoSegment {
                handle,
                ecn,
                count: 1,
                size: payload_len,
            });
        } else {
            // otherwise, release the message to the consumer
            self.release_message();
        }

        // let the caller know how big the payload was
        let outcome = tx::Outcome {
            len: payload_len,
            index: 0,
        };

        Ok(outcome)
    }

    #[inline]
    fn flush(&mut self) {
        // flush GSO segments between connections
        self.flush_gso();
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T: Message> Drop for TxQueue<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // flush the current GSO message, if possible
        self.flush_gso();
        // flush the pending messages for the channel
        self.flush_channel();
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Batch structure for reliable datagram transmission

use crate::{
    clock::{precision, wheel::SingleTimer},
    intrusive_queue::{Entry, Queue},
    packet::datagram::{
        partial::{PacketType, PartialDatagram},
        RoutingInfo,
    },
    socket::pool::descriptor,
};
use s2n_quic_core::varint::VarInt;
use std::net::SocketAddr;

/// Placeholder type for batches without an attached context (Send)
///
/// Holds a dangling NonNull so it's the same size as Rc<T> and matches its non-null
/// invariant for safe transmutation.
#[derive(Copy, Clone)]
pub struct NoContext(#[allow(dead_code)] std::ptr::NonNull<()>);

// SAFETY: NoContext is just a dangling pointer used for size/alignment.
// It never dereferences the pointer, so it's safe to send between threads.
unsafe impl Send for NoContext {}
unsafe impl Sync for NoContext {}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Ack = 0,
    FlowRetryReset = 1,
    FlowControl = 2,
    FlowData = 3,
    FlowInit = 4,
}

impl Priority {
    pub const LEVELS: usize = 5;

    #[inline]
    pub const fn as_index(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn from_datagram(datagram: &PartialDatagram) -> Self {
        match &datagram.packet_type {
            PacketType::Control { .. } => Self::Ack,
            PacketType::Datagram { routing_info, .. } => match routing_info {
                RoutingInfo::FlowValidateRequest { .. }
                | RoutingInfo::FlowInitValidate { .. }
                | RoutingInfo::FlowReset { .. } => Self::FlowRetryReset,
                RoutingInfo::FlowControl { .. } => Self::FlowControl,
                RoutingInfo::FlowData { .. } | RoutingInfo::None => Self::FlowData,
                RoutingInfo::FlowInit { .. } => Self::FlowInit,
            },
        }
    }
}

/// A batch of partial datagrams ready for transmission
///
/// Flows through the instance-wide wheel, sorted by transmission time.
/// All datagrams in a batch must go to the same peer (enforced at construction).
///
/// # Encoding Pipeline
/// 1. Application creates PartialDatagrams -> Batch
/// 2. Batch goes through wheel (timing)
/// 3. Batch distributed to socket workers
/// 4. **Encoder step**: Converts PartialDatagrams into encoded bytes
///    - Allocates packet numbers per datagram
///    - Gets sealer/credentials from path_secret_entry
///    - Encodes into GSO segments in storage buffer
/// 5. **Send step**: Transmits encoded bytes from storage
///
/// The `Ctx` parameter is NoContext by default (Send), or Rc<RefCell<PathContext>>
/// when attached to a worker-local path context (!Send).
#[repr(C)]
pub struct Batch<Ctx = NoContext> {
    /// Intrusive queue of partial datagrams
    pub datagrams: Queue<PartialDatagram>,
    /// Transmission time (1us intervals per sender)
    pub transmission_time: Option<precision::Timestamp>,
    /// Metadata for socket workers
    pub meta: Meta,
    /// Storage field for fully-encoded packets with GSO segment size
    pub encoded: Option<descriptor::Segments>,
    /// Optional path context (NoContext when Send, Rc when !Send)
    pub context: Ctx,
}

impl Batch<NoContext> {
    /// Creates a new batch with the given transmission time
    #[inline]
    pub fn new(transmission_time: Option<precision::Timestamp>, peer_addr: SocketAddr) -> Self {
        Self {
            datagrams: Queue::new(),
            transmission_time,
            meta: Meta {
                total_bytes: 0,
                peer_addr,
                starting_packet_number: None,
                is_probe: false,
                sender_id: VarInt::MAX, // Sentinel value - can be distributed via round-robin
                priority: Priority::FlowInit,
            },
            encoded: None,
            context: NoContext(std::ptr::NonNull::dangling()),
        }
    }
}

impl<Ctx> Batch<Ctx> {
    /// Pushes a datagram into this batch
    ///
    /// Updates total_bytes metadata.
    #[inline]
    pub fn push(&mut self, datagram: Entry<PartialDatagram>) {
        let datagram_priority = Priority::from_datagram(&datagram);
        if self.datagrams.is_empty() {
            self.meta.priority = datagram_priority;
        } else {
            debug_assert_eq!(
                self.meta.priority, datagram_priority,
                "Batch priority mismatch: existing={:?}, new={:?}",
                self.meta.priority, datagram_priority
            );
        }

        let len = datagram.estimate_encoded_len(16);
        // TODO assert it fits into u16
        // TODO assert the total_len doesn't overflow
        // TODO assert that the len is less than or equal to the existing datagrams, if any
        self.meta.total_bytes += len as u16;
        self.datagrams.push_back(datagram);
    }

    /// Sets the sticky sender_id for this batch
    ///
    /// Used to ensure FlowInit/FlowInitRetry packets always originate from the same sender.
    /// Set to VarInt::MAX (default) for regular packets that can be round-robin distributed.
    #[inline]
    pub fn with_sender_id(mut self, sender_id: VarInt) -> Self {
        debug_assert!(
            self.meta.sender_id == VarInt::MAX || self.meta.sender_id == sender_id,
            "Batch sender_id mismatch: existing={:?}, new={:?}",
            self.meta.sender_id,
            sender_id
        );
        self.meta.sender_id = sender_id;
        self
    }

    /// Returns true if the batch is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.datagrams.is_empty()
    }

    /// Returns the number of datagrams in the batch
    #[inline]
    pub fn len(&self) -> usize {
        self.datagrams.len()
    }
}

impl<Sealer> Batch<std::rc::Rc<std::cell::RefCell<crate::socket::channel::PathContext<Sealer>>>>
where
    Sealer: crate::crypto::seal::Application,
{
    /// Encode this batch into the provided descriptor
    pub fn encode(
        &mut self,
        unfilled: descriptor::Unfilled,
        source_control_port: u16,
        source_sender_id: VarInt,
    ) {
        use s2n_codec::EncoderBuffer;

        // Verify sticky sender_id consistency
        debug_assert!(
            self.meta.sender_id == VarInt::MAX || self.meta.sender_id == source_sender_id,
            "Sticky batch on wrong sender: batch sender_id={:?}, socket sender_id={:?}",
            self.meta.sender_id,
            source_sender_id
        );

        // Borrow context for the entire encoding operation
        let mut ctx = self.context.borrow_mut();
        let ctx = &mut *ctx;

        // Get references to sealer and credentials
        let sealer = &ctx.sealer;
        let credentials = &ctx.credentials;
        let flow_attempt_id = &mut ctx.flow_attempt_id_counter;

        let mut segment_size = 0;

        // Encode all datagrams into the descriptor
        let result = unfilled.fill_with(|addr, _cmsg, mut payload| {
            let mut offset = 0usize;
            let mut watermark = 0usize;
            let mut packet_number = ctx.next_packet_number;

            // If the batch contains probes then we need to cause a gap for an immediate ACK
            if self.meta.is_probe {
                packet_number += 5;
            }

            // Store starting packet number in batch metadata for registration phase
            self.meta.starting_packet_number = Some(packet_number);

            for dgram in self.datagrams.iter_mut() {
                // Estimate segment size from first datagram if not set
                if segment_size == 0 {
                    segment_size = dgram.estimate_encoded_len(16);
                }

                // Zero out any padding bytes between packets
                if offset > watermark {
                    payload[watermark..offset].fill(0);
                }

                // Create encoder buffer for this transmission
                let buf = &mut payload[offset..];
                let encoder_buf = EncoderBuffer::new(buf);

                // Encode the transmission (datagram or control packet)
                let encoded_len = dgram.encode(
                    encoder_buf,
                    source_control_port,
                    source_sender_id,
                    packet_number,
                    sealer,
                    credentials,
                    flow_attempt_id,
                );

                watermark = offset + encoded_len;
                packet_number += 1;

                // Move offset to the next segment boundary for GSO
                // We need uniform segment sizes for GSO to work correctly
                offset += segment_size;
            }

            // Update context with new packet number
            ctx.next_packet_number = packet_number;

            // Set remote address
            addr.set(self.meta.peer_addr.into());

            <Result<_, core::convert::Infallible>>::Ok(watermark)
        });

        let segments = result.expect("fill_with doesn't fail");

        // Store encoded data in batch
        let filled = segments.take_filled();
        self.encoded = Some(descriptor::Segments::new(filled, segment_size as u16));
    }
}

/// Builder for constructing batches with GSO constraints
///
/// Maintains state to enforce uniform segment sizes and other GSO requirements.
pub struct Builder {
    /// The batch being built
    batch: Batch,
    /// The uniform segment size (all segments except last must match this)
    segment_size: Option<u16>,
    /// Whether the last segment added was undersized (smaller than uniform size)
    /// Once true, no more segments can be added (GSO requires last segment to be final)
    has_undersized_segment: bool,
}

impl Builder {
    /// Creates a new batch builder
    #[inline]
    pub fn new(transmission_time: Option<precision::Timestamp>, peer_addr: SocketAddr) -> Self {
        Self {
            batch: Batch::new(transmission_time, peer_addr),
            segment_size: None,
            has_undersized_segment: false,
        }
    }

    /// Tries to push a datagram into this batch, checking GSO constraints
    ///
    /// Returns `Ok(())` if the datagram was added, or `Err(datagram)` if it couldn't
    /// be added due to batch constraints.
    ///
    /// # Constraints checked:
    /// - Maximum segment count (GSO limit)
    /// - Maximum total payload size (sendmsg limit)
    /// - Uniform segment size (GSO requires all segments same size except last)
    /// - No segments after an undersized segment (GSO requires last segment to be final)
    /// - Destination address match
    /// - Sticky sender_id match (for FlowInit/FlowInitRetry packets)
    /// - Priority match (all datagrams in a batch must share the same priority)
    pub fn try_push(
        &mut self,
        datagram: Entry<PartialDatagram>,
    ) -> Result<(), Entry<PartialDatagram>> {
        use crate::msg::segment;

        // If we already added an undersized segment, we can't add more
        // GSO requires the undersized segment to be the final one
        if self.has_undersized_segment {
            return Err(datagram);
        }

        let datagram_priority = Priority::from_datagram(&datagram);
        if self.batch.datagrams.is_empty() {
            self.batch.meta.priority = datagram_priority;
        } else if self.batch.meta.priority != datagram_priority {
            return Err(datagram);
        }

        let len = datagram.estimate_encoded_len(16);

        // Check if we've hit the maximum segment count
        let current_count = self.batch.datagrams.len();
        if current_count >= segment::MAX_COUNT {
            return Err(datagram);
        }

        // Check if adding this would exceed the maximum total payload size
        let new_total = self.batch.meta.total_bytes as u32 + len as u32;
        if new_total > segment::MAX_TOTAL as u32 {
            return Err(datagram);
        }

        // Check destination address matches
        if datagram.remote_address() != self.batch.meta.peer_addr {
            return Err(datagram);
        }

        // Check sticky sender_id compatibility
        // If this datagram requires a specific sender_id and the batch already has one set,
        // they must match
        let sticky_sender_id = datagram.sticky_sender_id();
        if let Some(dgram_sender_id) = sticky_sender_id {
            if self.batch.meta.sender_id != VarInt::MAX
                && self.batch.meta.sender_id != dgram_sender_id
            {
                return Err(datagram);
            }
        }

        // GSO requires uniform segment sizes (except the last segment can be smaller)
        if let Some(expected_size) = self.segment_size {
            // We already have a uniform size established
            // New segment must either match it or be smaller (final segment)
            if len > expected_size as usize {
                return Err(datagram);
            }
            // Mark if this segment is undersized
            if len < expected_size as usize {
                self.has_undersized_segment = true;
            }
        } else {
            // This is the first segment, establish the uniform size
            // Clamp to u16 since segment sizes must fit in u16
            self.segment_size = Some(len.min(u16::MAX as usize) as u16);
        }

        // Set sticky sender_id if this datagram requires it
        if let Some(sender_id) = sticky_sender_id {
            self.batch.meta.sender_id = sender_id;
        }

        // All constraints satisfied, add to batch
        self.batch.meta.total_bytes += len as u16;
        self.batch.datagrams.push_back(datagram);
        Ok(())
    }

    /// Sets the sticky sender_id for this batch
    ///
    /// Used to ensure FlowInit/FlowInitRetry packets always originate from the same sender
    /// during retransmission.
    #[inline]
    pub fn set_sender_id(&mut self, sender_id: VarInt) {
        debug_assert!(
            self.batch.meta.sender_id == VarInt::MAX || self.batch.meta.sender_id == sender_id,
            "Builder sender_id mismatch: existing={:?}, new={:?}",
            self.batch.meta.sender_id,
            sender_id
        );
        self.batch.meta.sender_id = sender_id;
    }

    /// Finishes building and returns the batch
    #[inline]
    pub fn finish(self) -> Batch {
        self.batch
    }

    /// Returns true if the batch is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Returns the number of datagrams in the batch
    #[inline]
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Returns the current batch metadata
    #[inline]
    pub fn meta(&self) -> &Meta {
        &self.batch.meta
    }
}

impl<Ctx> SingleTimer for Batch<Ctx> {
    #[inline]
    fn target_time(&self) -> Option<precision::Timestamp> {
        self.transmission_time
    }

    #[inline]
    fn set_target_time(&mut self, time: precision::Timestamp) {
        self.transmission_time = Some(time);
    }
}

impl<Ctx> crate::socket::channel::ByteCost for Batch<Ctx> {
    fn byte_cost(&self) -> u64 {
        self.meta.total_bytes as u64
    }
}

// TODO: Implement encoding pipeline for Batch
// Steps needed:
// 1. Create encoder adapter in socket pipeline (before Sendable)
// 2. Encoder allocates packet numbers per datagram (per-peer counter)
// 3. Gets sealer/credentials from path_secret_entry
// 4. Encodes each PartialDatagram into GSO segments
// 5. Stores encoded bytes in Batch.encoded field
// 6. Sendable impl just transmits from Batch.encoded storage

impl<Ctx> crate::socket::channel::Sendable for Batch<Ctx> {
    fn send<S: crate::socket::send::Socket>(&mut self, socket: &S) -> std::io::Result<()> {
        self.encoded
            .as_mut()
            .expect("batch must be encoded before sending")
            .send(socket)
    }
}

/// Metadata about a batch for socket workers
#[derive(Clone, Debug)]
pub struct Meta {
    /// Total bytes in all datagrams (for rate limiting)
    pub total_bytes: u16,
    /// Destination peer address
    pub peer_addr: SocketAddr,
    /// Starting packet number for this batch (datagrams numbered contiguously)
    pub starting_packet_number: Option<VarInt>,
    /// Whether this batch is a probe (skips packet numbers to elicit immediate ACK)
    pub is_probe: bool,
    /// Sticky sender ID for flow initialization packets.
    ///
    /// Set to VarInt::MAX (sentinel value) for regular packets that can be distributed
    /// via round-robin. Set to a specific sender_id for FlowInit/FlowInitRetry packets
    /// that must always originate from the same sender.
    pub sender_id: VarInt,
    /// Batch transmission priority used by endpoint priority wheels.
    pub priority: Priority,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        byte_vec::ByteVec,
        packet::{self, datagram::RoutingInfo},
        path::secret::map::Entry,
    };

    #[test]
    fn batch_creation() {
        let batch = Batch::new(None, "127.0.0.1:8080".parse().unwrap());
        assert!(batch.is_empty());
        assert_eq!(batch.len(), 0);
        assert_eq!(batch.meta.total_bytes, 0);
        assert_eq!(batch.meta.priority, Priority::FlowInit);
    }

    #[test]
    fn batch_push() {
        let mut batch = Batch::new(None, "127.0.0.1:8080".parse().unwrap());
        let entry = Entry::fake("127.0.0.1:8080".parse().unwrap(), None);

        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"test"));

        let datagram = PartialDatagram::new_datagram(
            RoutingInfo::None,
            ByteVec::new(),
            payload,
            entry,
            None.into(),
        );

        batch.push(datagram.into());
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.meta.total_bytes, 4); // payload only
        assert_eq!(batch.meta.priority, Priority::FlowData);
    }

    #[test]
    fn batch_scheduled() {
        let mut batch = Batch::new(None, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(batch.target_time(), None);

        let time = precision::Timestamp { nanos: 1000000 }; // 1ms in nanos
        batch.set_target_time(time);
        assert_eq!(batch.target_time(), Some(time));
    }

    #[test]
    fn builder_rejects_priority_mismatch() {
        let entry = Entry::fake("127.0.0.1:8080".parse().unwrap(), None);

        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"test"));
        let datagram = PartialDatagram::new_datagram(
            RoutingInfo::None,
            ByteVec::new(),
            payload,
            entry.clone(),
            None.into(),
        );
        let control = PartialDatagram::new_control(
            packet::control::RoutingInfo::Sender {
                source_sender_id: VarInt::from_u8(1),
                dest_sender_id: VarInt::from_u8(2),
            },
            ByteVec::new(),
            entry,
        );

        let mut builder = Builder::new(None, "127.0.0.1:8080".parse().unwrap());
        assert!(builder.try_push(datagram.into()).is_ok());
        assert!(builder.try_push(control.into()).is_err());
    }
}

// ── Context Attachment ──────────────────────────────────────────────────────

// Compile-time assertions to verify Batch<NoContext> and Batch<Rc<T>> are layout-compatible
const _: () = {
    use core::mem::{align_of, offset_of, size_of};

    // Use a concrete type for Rc since we can't use generics in const
    type RcType = std::rc::Rc<std::cell::RefCell<()>>;

    const fn assert_layout_compatible() {
        // NoContext must have the same size and alignment as Rc<T>
        assert!(size_of::<NoContext>() == size_of::<RcType>());
        assert!(align_of::<NoContext>() == align_of::<RcType>());

        // Batch<NoContext> and Batch<Rc<T>> must have the same size and alignment
        assert!(size_of::<Batch<NoContext>>() == size_of::<Batch<RcType>>());
        assert!(align_of::<Batch<NoContext>>() == align_of::<Batch<RcType>>());

        // All fields before 'context' must have the same offset
        assert!(offset_of!(Batch<NoContext>, datagrams) == offset_of!(Batch<RcType>, datagrams));
        assert!(
            offset_of!(Batch<NoContext>, transmission_time)
                == offset_of!(Batch<RcType>, transmission_time)
        );
        assert!(offset_of!(Batch<NoContext>, meta) == offset_of!(Batch<RcType>, meta));
        assert!(offset_of!(Batch<NoContext>, encoded) == offset_of!(Batch<RcType>, encoded));

        // The context field itself must be at the same offset
        assert!(offset_of!(Batch<NoContext>, context) == offset_of!(Batch<RcType>, context));
    }

    assert_layout_compatible();
};

impl Entry<Batch<NoContext>> {
    /// Attach a context to this batch, converting it to a worker-local batch
    ///
    /// # Safety
    /// This transmutes Entry<Batch<NoContext>> into Entry<Batch<Rc<T>>>.
    /// The batch becomes !Send and must stay on the worker thread.
    pub fn with_context<T>(mut self, context: std::rc::Rc<T>) -> Entry<Batch<std::rc::Rc<T>>> {
        unsafe {
            // SAFETY: Transmute Rc<T> directly to NoContext to copy its internal pointer
            // representation (NonNull<RcBox<T>>) without affecting reference counts.
            // Note: Rc::into_raw() cannot be used here as it returns a pointer to T,
            // but we need to preserve the pointer to RcBox<T> that Rc contains internally.
            self.context = std::mem::transmute(context);

            // Now transmute the entire Entry - the context field contains valid Rc bits
            std::mem::transmute(self)
        }
    }
}

impl<T> Entry<Batch<std::rc::Rc<T>>> {
    /// Split this batch back into a Send batch and its context
    ///
    /// # Safety
    /// This transmutes Entry<Batch<Rc<T>>> back into Entry<Batch<NoContext>>.
    /// After this call, the batch is Send again.
    pub fn into_parts(mut self) -> (Entry<Batch<NoContext>>, std::rc::Rc<T>) {
        unsafe {
            // Extract the Rc<T> by transmuting directly from the context field
            // This preserves the internal RcBox pointer correctly
            let context: std::rc::Rc<T> = std::mem::transmute_copy(&self.context);

            // Transmute self to NoContext version first (avoids creating invalid Rc)
            let mut batch: Entry<Batch<NoContext>> = std::mem::transmute(self);

            // Now write the dangling pointer (safe because batch.context is NoContext now)
            batch.context = NoContext(std::ptr::NonNull::dangling());

            (batch, context)
        }
    }
}

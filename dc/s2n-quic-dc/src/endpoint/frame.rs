// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Frame: the application's unit of work for the reliable datagram pipeline.
//!
//! A Frame decouples the application's unit of work from the transport's unit of work (packets).
//! Writers produce Frames; the transport layer decides how to pack multiple Frames into packets,
//! encrypt them, and transmit them. For small payloads, many Frames pack into a single packet.
//! For large payloads, a single Frame fills an entire packet by itself.
//!
//! The Frame carries a Header (routing metadata), a payload, completion notification, TTL for
//! bounded retransmission, and a target transmission time for pacing. Transport-assigned fields
//! like source_sender_id and attempt_id live in the Header as mutable slots that the Peer
//! Context fills during packet assembly.

use crate::{
    byte_vec::ByteVec,
    intrusive::{Entry, Queue},
    packet::datagram::{QueuePair, ResetTarget},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive::datagram_completion, ByteCost, UnboundedSender},
    time::precision,
};
use s2n_codec::{decoder_invariant, Encoder, EncoderValue};
use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};
use std::sync::Arc;

/// Default TTL for frames (number of transmission attempts before failure).
pub const DEFAULT_TTL: u16 = u16::MAX;

/// Worst-case header overhead for a QueueData packet on the wire.
pub const MAX_QUEUE_DATA_HEADER_OVERHEAD: u16 = 111;

/// Worst-case header overhead for a QueueMsg packet on the wire.
///
/// QueueMsg encodes three additional VarInt fields (msg_id, stream_offset, message_size)
/// compared to QueueData, adding up to 24 bytes in the worst case.
pub const MAX_QUEUE_MSG_HEADER_OVERHEAD: u16 = 144;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    QueueReset = 0,
    QueueControl = 1,
    QueueData = 2,
    QueueInit = 3,
}

impl Priority {
    pub const LEVELS: usize = 4;
    pub const ALL: [Self; Self::LEVELS] = [
        Self::QueueReset,
        Self::QueueControl,
        Self::QueueData,
        Self::QueueInit,
    ];

    #[inline]
    pub const fn as_index(self) -> usize {
        self as usize
    }
}

/// Completion channel sender typed on Frame.
pub type CompletionSender = datagram_completion::Sender<Frame>;

/// Completion channel receiver typed on Frame.
pub type CompletionReceiver = datagram_completion::Receiver<Frame>;

/// Stack-allocated sender input for the frame submission channel.
///
/// Callers (readers, writers, dispatch) create a `PriorityInput` on the stack, insert frames
/// via [`push`], and submit it with [`SubmissionSender::send_batch`]. No heap allocation is
/// needed on the submission path.
///
/// Inside the sharded channel, each shard accumulates multiple `PriorityInput` values
/// by appending them into its [`PriorityStorage`] (Box-backed). The receiver then
/// pointer-swaps the Box in O(1) to obtain the shard's accumulated queues.
///
/// [`push`]: PriorityInput::push
/// [`SubmissionSender::send_batch`]: crate::socket::channel::intrusive_queue::sharded::Sender::send_batch
pub struct PriorityInput {
    queues: [Queue<Frame>; Priority::LEVELS],
    len: usize,
}

impl Default for PriorityInput {
    fn default() -> Self {
        Self {
            queues: std::array::from_fn(|_| Queue::new()),
            len: 0,
        }
    }
}

impl PriorityInput {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Inserts `frame` into the priority bucket matching its [`Frame::priority`].
    #[inline]
    pub fn push(&mut self, frame: Entry<Frame>) {
        let idx = frame.priority().as_index();
        self.queues[idx].push_back(frame);
        self.len += 1;
    }

    /// Iterates over all frames across all priority buckets, highest priority first.
    pub fn iter(&self) -> impl Iterator<Item = &Frame> {
        self.queues.iter().flat_map(|q| q.iter())
    }
}

impl core::fmt::Debug for PriorityInput {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut list = f.debug_list();
        for frame in self.iter() {
            list.entry(frame);
        }
        list.finish()
    }
}

impl UnboundedSender<Entry<Frame>> for PriorityInput {
    #[inline]
    fn send(&mut self, value: Entry<Frame>) -> Result<(), Entry<Frame>> {
        self.push(value);
        Ok(())
    }
}

/// Box-backed shard-local storage for the frame submission channel.
///
/// Each shard holds one `PriorityStorage`, which is a heap-allocated array of
/// [`Priority::LEVELS`] intrusive queues.  Because only the Box pointer is swapped during
/// [`poll_swap`] — not the entire queue array — the swap is O(1) regardless of how many
/// frames are buffered or how many priority levels exist.
///
/// Senders submit [`PriorityInput`] values (stack-allocated); the shard's `append` method
/// merges those stack queues into this Box in O([`Priority::LEVELS`]) list-append operations.
///
/// [`poll_swap`]: crate::socket::channel::intrusive_queue::sharded::Receiver::poll_swap
#[derive(Default)]
pub struct PriorityStorage(Box<PriorityInput>);

impl PriorityStorage {
    pub fn drain(&mut self) -> impl Iterator<Item = (Priority, Queue<Frame>)> + '_ {
        self.0.len = 0;
        Priority::ALL
            .iter()
            .zip(self.0.queues.iter_mut())
            .map(|(&priority, queue)| (priority, core::mem::take(queue)))
    }
}

impl crate::socket::channel::intrusive::sharded::Storage<crate::intrusive::EntryAdapter<Frame>>
    for PriorityStorage
{
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl
    crate::socket::channel::intrusive::sharded::Input<
        crate::intrusive::EntryAdapter<Frame>,
        PriorityStorage,
    > for PriorityInput
{
    #[inline(always)]
    fn is_empty(&self) -> bool {
        PriorityInput::is_empty(self)
    }

    #[inline(always)]
    fn append_to(mut self, storage: &mut PriorityStorage) {
        storage.0.len += self.len;
        self.len = 0;
        for (dst, src) in storage.0.queues.iter_mut().zip(self.queues.iter_mut()) {
            dst.append(src);
        }
    }
}

impl
    crate::socket::channel::intrusive::sharded::Input<
        crate::intrusive::EntryAdapter<Frame>,
        PriorityStorage,
    > for &mut PriorityInput
{
    #[inline(always)]
    fn is_empty(&self) -> bool {
        PriorityInput::is_empty(self)
    }

    #[inline(always)]
    fn append_to(self, storage: &mut PriorityStorage) {
        storage.0.len += self.len;
        self.len = 0;
        for (dst, src) in storage.0.queues.iter_mut().zip(self.queues.iter_mut()) {
            dst.append(src);
        }
    }
}

impl
    crate::socket::channel::intrusive::sharded::Input<
        crate::intrusive::EntryAdapter<Frame>,
        PriorityStorage,
    > for Entry<Frame>
{
    #[inline(always)]
    fn is_empty(&self) -> bool {
        false
    }

    #[inline(always)]
    fn append_to(self, storage: &mut PriorityStorage) {
        let idx = self.priority().as_index();
        storage.0.queues[idx].push_back(self);
        storage.0.len += 1;
    }
}
/// A batch of frames that all share the same priority level.
///
/// Unlike [`PriorityInput`], which fans frames into per-priority buckets at push time,
/// `HomogeneousBatch` wraps a single [`Queue<Frame>`] whose frames are all known to have
/// the same priority.  The `append_to` implementation performs a single O(1) list-append
/// into the correct priority bucket, avoiding an unnecessary O([`Priority::LEVELS`])
/// iteration.
///
/// Use this when the caller knows the priority of every frame at construction time —
/// for example, the stream writer which always produces [`Priority::QueueData`] frames.
pub struct HomogeneousBatch {
    pub queue: Queue<Frame>,
    pub priority: Priority,
}

impl
    crate::socket::channel::intrusive::sharded::Input<
        crate::intrusive::EntryAdapter<Frame>,
        PriorityStorage,
    > for HomogeneousBatch
{
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    #[inline(always)]
    fn append_to(mut self, storage: &mut PriorityStorage) {
        let len = self.queue.len();
        if len == 0 {
            return;
        }
        debug_assert!(
            self.queue
                .iter()
                .all(|frame| frame.priority() == self.priority),
            "HomogeneousBatch priority did not match all queued frames"
        );
        let idx = self.priority.as_index();
        storage.0.queues[idx].append(&mut self.queue);
        storage.0.len += len;
    }
}

impl PriorityStorage {
    /// Iterates over all frames across all priority buckets, highest priority first.
    pub fn iter(&self) -> impl Iterator<Item = &Frame> {
        self.0.queues.iter().flat_map(|q| q.iter())
    }
}

impl core::fmt::Debug for PriorityStorage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut list = f.debug_list();
        for frame in self.iter() {
            list.entry(frame);
        }
        list.finish()
    }
}

/// Submission channel sender typed on Frame.
pub type SubmissionSender = crate::socket::channel::intrusive::sharded::Sender<
    crate::intrusive::EntryAdapter<Frame>,
    PriorityStorage,
>;

/// Submission channel receiver typed on Frame.
pub type SubmissionReceiver = crate::socket::channel::intrusive::sharded::Receiver<
    crate::intrusive::EntryAdapter<Frame>,
    PriorityStorage,
>;

/// Creates a new frame submission channel.
///
/// `shard_count` must be a power of two. More shards reduce contention between concurrent senders
/// at the cost of receiver bookkeeping. A good default is the number of workers rounded up to the
/// next power of two, multiplied by a small constant (e.g. 4).
pub fn submission_channel(shard_count: usize) -> (SubmissionSender, SubmissionReceiver) {
    crate::socket::channel::intrusive::sharded::new_with_storage::<Frame, PriorityStorage>(
        shard_count,
    )
}

/// Create a new completion channel for Frames.
pub fn completion_channel() -> CompletionReceiver {
    datagram_completion::new_with_mode(datagram_completion::SubscriptionMode::All)
}

/// Create a new completion channel that only delivers failed transmissions.
pub fn failure_completion_channel() -> CompletionReceiver {
    datagram_completion::new_with_mode(datagram_completion::SubscriptionMode::FailuresOnly)
}

/// Status of a frame's transmission through the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransmissionStatus {
    /// Frame is pending transmission or in flight
    #[default]
    Pending,
    /// Frame was acknowledged by the peer
    Acknowledged,
    /// Frame failed to be delivered
    Failed(FailureReason),
}

/// Reasons why a frame might fail to be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// The peer was declared dead (PTO reached max idle time)
    PeerDead,
    /// Transmission error (peer active but refused packet)
    TransmissionError,
    /// Unknown path secret (path secret refused by peer)
    UnknownPathSecret,
    /// The sender was dropped and requested cancellation
    Cancelled,
}

/// Routing metadata for a Frame.
///
/// Describes what kind of frame this is and the per-frame routing fields. The
/// source_sender_id is NOT here — it lives on the Frame struct and gets stamped
/// into the packet header at encryption time by the peer context.
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Header {
    /// Stream data routed via queue pair.
    ///
    /// When `dest_acceptor_id` is `Some`, this is an "init" frame that can create
    /// a new binding on the server if the queue is unbound. Once the server confirms
    /// the binding (via MaxData), subsequent frames omit the acceptor_id.
    ///
    /// `priority` is encoded on the wire only for init frames; it tells the server
    /// which credit tier the inbound stream should park on. The peer remembers the
    /// priority after binding so subsequent frames carry no priority on the wire.
    /// On non-init frames this field is informational only and is set to a default
    /// after decode.
    QueueData {
        queue_pair: QueuePair,
        binding_id: VarInt,
        offset: VarInt,
        is_fin: bool,
        dest_acceptor_id: Option<VarInt>,
        priority: crate::credit::Priority,
    },
    /// Flow control (MAX_DATA and other control frames)
    QueueControl {
        queue_pair: QueuePair,
        binding_id: VarInt,
    },
    /// Inline window update: MAX_DATA value carried directly in the header.
    ///
    /// This is the fast path for the common case of the reader advertising a
    /// new receive window to the writer.  Carrying `maximum_data` in the header
    /// avoids encoding and decoding an opaque QUIC control-frame payload. The
    /// payload for this frame type is always empty.
    ///
    /// The generic [`QueueControl`] variant with an opaque payload remains
    /// available as an extension escape hatch for future multi-frame or
    /// non-MAX_DATA control messages.
    QueueMaxData {
        queue_pair: QueuePair,
        binding_id: VarInt,
        maximum_data: VarInt,
    },
    /// Reset a flow
    QueueReset {
        dest_queue_id: VarInt,
        binding_id: VarInt,
        reset_target: ResetTarget,
        error_code: VarInt,
        dest_acceptor_id: Option<VarInt>,
    },
    /// Free queue slots (server→client credit return).
    ///
    /// Uses sorted delta encoding in the payload. The free_request_id is
    /// a monotonic stamp for receiver-side dedup of replayed/duplicate frames.
    QueueFree {
        free_request_id: VarInt,
        smallest_queue_id: VarInt,
    },
    /// ACK frame with all metadata lifted into the header.
    ///
    /// The first ACK range (`ack_range..=largest_acknowledged`) and ECN counts live
    /// entirely in the header. The body contains only additional gap/range pairs when
    /// there are gaps (loss). In the common no-loss case, the body is empty.
    ///
    /// ack_delay is computed by the sender at assembly time as `now - largest_recv_time`,
    /// giving the most accurate delay measurement possible.
    Ack {
        dest_sender_id: VarInt,
        ack_delay: VarInt,
        largest_acknowledged: VarInt,
        ack_range: VarInt,
        ecn_counts: EcnCounts,
        is_ack_eliciting: bool,
    },
    /// Pre-allocated message data routed via queue pair.
    ///
    /// Unlike QueueData which allocates per-frame, QueueMsg enables the receiver to
    /// pre-allocate a single contiguous buffer for the entire message and decrypt/copy
    /// frame payloads directly into it at the correct offset. The receiver only wakes
    /// the application once the full message is assembled.
    ///
    /// Every frame is self-describing: it carries msg_id + message_size so the receiver
    /// can allocate on any frame regardless of arrival order. The offset is message-local
    /// (0-based).
    QueueMsg {
        queue_pair: QueuePair,
        binding_id: VarInt,
        msg_id: VarInt,
        stream_offset: VarInt,
        message_size: VarInt,
        chunk_size: VarInt,
        chunk_index: VarInt,
        is_fin: bool,
        is_wakeup: bool,
        dest_acceptor_id: Option<VarInt>,
        /// Encoded on the wire only when `dest_acceptor_id.is_some()`. See `QueueData`.
        priority: crate::credit::Priority,
    },
    /// Minimal ack-eliciting frame with no payload or fields.
    ///
    /// Used as the second PTO probe segment when there is only one inflight entry
    /// to retransmit. Ensures the peer receives 2 ack-eliciting packets at contiguous
    /// packet numbers, satisfying the PN-threshold for loss detection.
    Ping,
}

impl Header {
    const QUEUE_DATA_NO_FIN_TYPE: u8 = 1;
    const QUEUE_DATA_WITH_FIN_TYPE: u8 = 2;
    const QUEUE_DATA_INIT_NO_FIN_TYPE: u8 = 3;
    const QUEUE_DATA_INIT_WITH_FIN_TYPE: u8 = 4;
    const QUEUE_CONTROL_TYPE: u8 = 5;
    const QUEUE_MAX_DATA_TYPE: u8 = 6;
    const QUEUE_RESET_BOTH_TYPE: u8 = 7;
    const QUEUE_RESET_STREAM_TYPE: u8 = 8;
    const QUEUE_RESET_CONTROL_TYPE: u8 = 9;
    const QUEUE_RESET_BOTH_INIT_TYPE: u8 = 10;
    const QUEUE_RESET_STREAM_INIT_TYPE: u8 = 11;
    const QUEUE_RESET_CONTROL_INIT_TYPE: u8 = 12;
    const QUEUE_FREE_TYPE: u8 = 13;
    const ACK_TYPE: u8 = 14;
    const ACK_ELICITING_TYPE: u8 = 15;
    // QueueMsg: 8 type tags with bit-positioned flags.
    // Bit 0: is_fin, Bit 1: is_wakeup, Bit 2: has_dest_acceptor_id (init)
    const QUEUE_MSG_BASE_TYPE: u8 = 16;
    const QUEUE_MSG_MAX_TYPE: u8 = Self::QUEUE_MSG_BASE_TYPE + 7;
    const PING_TYPE: u8 = 24;

    #[inline]
    pub fn priority(&self) -> Priority {
        match self {
            Self::QueueData {
                dest_acceptor_id: Some(_),
                ..
            }
            | Self::QueueMsg {
                dest_acceptor_id: Some(_),
                ..
            } => Priority::QueueInit,
            Self::QueueData { .. } | Self::QueueMsg { .. } => Priority::QueueData,
            Self::QueueControl { .. } | Self::QueueMaxData { .. } => Priority::QueueControl,
            Self::QueueFree { .. } | Self::Ack { .. } | Self::QueueReset { .. } | Self::Ping => {
                Priority::QueueReset
            }
        }
    }

    /// Returns the wire-canonical form of `self`: clears fields that the encoder
    /// drops when their qualifying flag is absent, so that `encode-then-decode` is
    /// equality-preserving. Currently only `priority` (encoded only on init frames)
    /// is affected.
    ///
    /// This exists primarily for fuzz/property tests that synthesize arbitrary
    /// `Header` values; production code never produces a non-canonical header.
    #[inline]
    pub fn canonicalize_for_wire(self) -> Self {
        match self {
            Self::QueueData {
                queue_pair,
                binding_id,
                offset,
                is_fin,
                dest_acceptor_id,
                priority,
            } => Self::QueueData {
                queue_pair,
                binding_id,
                offset,
                is_fin,
                dest_acceptor_id,
                priority: if dest_acceptor_id.is_some() {
                    priority
                } else {
                    crate::credit::Priority::default()
                },
            },
            Self::QueueMsg {
                queue_pair,
                binding_id,
                msg_id,
                stream_offset,
                message_size,
                chunk_size,
                chunk_index,
                is_fin,
                is_wakeup,
                dest_acceptor_id,
                priority,
            } => Self::QueueMsg {
                queue_pair,
                binding_id,
                msg_id,
                stream_offset,
                message_size,
                chunk_size,
                chunk_index,
                is_fin,
                is_wakeup,
                dest_acceptor_id,
                priority: if dest_acceptor_id.is_some() {
                    priority
                } else {
                    crate::credit::Priority::default()
                },
            },
            other => other,
        }
    }

    /// Returns true if a frame with this header type elicits an ACK from the peer.
    ///
    /// ACK frames are not ack-eliciting by default. When `is_ack_eliciting` is set
    /// on an ACK frame (acting as a PING), it triggers the peer to acknowledge the
    /// packet. All other frame types are always ack-eliciting.
    #[inline]
    pub fn is_ack_eliciting(&self) -> bool {
        match self {
            Self::Ack {
                is_ack_eliciting, ..
            } => *is_ack_eliciting,
            _ => true,
        }
    }

    /// Returns true if this header variant carries a per-frame payload length entry.
    ///
    /// This indicates whether the application header includes an explicit payload length for the
    /// frame, not whether the frame must contain non-empty payload bytes. Control and flow-control
    /// frames can legitimately encode a zero-length payload while still carrying the length field.
    #[inline]
    pub fn has_payload_length(&self) -> bool {
        match self {
            Self::QueueData { .. }
            | Self::QueueMsg { .. }
            | Self::QueueControl { .. }
            | Self::QueueFree { .. }
            | Self::Ack { .. } => true,
            Self::QueueReset { .. } | Self::QueueMaxData { .. } | Self::Ping => false,
        }
    }

    /// Returns the number of bytes this header occupies in the application header region,
    /// including the optional payload-length varint when [`has_payload_length`] is true.
    ///
    /// This is the single canonical implementation of the frame-metadata size calculation.
    /// `assemble::frame_metadata_len` delegates here so both callers operate on the same
    /// assumptions. Debug builds assert that header variants without a payload-length field
    /// always receive an empty payload.
    #[inline]
    pub fn metadata_len(&self, payload_len: usize) -> usize {
        if self.has_payload_length() {
            let payload_len_varint = VarInt::try_from(payload_len as u64).unwrap_or(VarInt::ZERO);
            self.encoding_size() + payload_len_varint.encoding_size()
        } else {
            debug_assert_eq!(
                payload_len, 0,
                "frames without payload_length must have zero payload"
            );
            self.encoding_size()
        }
    }
}

impl EncoderValue for Header {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        match self {
            Self::QueueData {
                queue_pair,
                binding_id,
                offset,
                is_fin,
                dest_acceptor_id,
                priority,
            } => {
                let tag = match (dest_acceptor_id.is_some(), *is_fin) {
                    (true, false) => Self::QUEUE_DATA_INIT_NO_FIN_TYPE,
                    (true, true) => Self::QUEUE_DATA_INIT_WITH_FIN_TYPE,
                    (false, false) => Self::QUEUE_DATA_NO_FIN_TYPE,
                    (false, true) => Self::QUEUE_DATA_WITH_FIN_TYPE,
                };
                encoder.encode(&tag);
                encoder.encode(queue_pair);
                if let Some(acceptor_id) = dest_acceptor_id {
                    encoder.encode(acceptor_id);
                    encoder.encode(&priority.as_u8());
                }
                encoder.encode(binding_id);
                encoder.encode(offset);
            }
            Self::QueueControl {
                queue_pair,
                binding_id,
            } => {
                encoder.encode(&Self::QUEUE_CONTROL_TYPE);
                encoder.encode(queue_pair);
                encoder.encode(binding_id);
            }
            Self::QueueMaxData {
                queue_pair,
                binding_id,
                maximum_data,
            } => {
                encoder.encode(&Self::QUEUE_MAX_DATA_TYPE);
                encoder.encode(queue_pair);
                encoder.encode(binding_id);
                encoder.encode(maximum_data);
            }
            Self::QueueReset {
                dest_queue_id,
                binding_id,
                reset_target,
                error_code,
                dest_acceptor_id,
            } => {
                let reset_type = match (dest_acceptor_id.is_some(), reset_target) {
                    (false, ResetTarget::Both) => Self::QUEUE_RESET_BOTH_TYPE,
                    (false, ResetTarget::Stream) => Self::QUEUE_RESET_STREAM_TYPE,
                    (false, ResetTarget::Control) => Self::QUEUE_RESET_CONTROL_TYPE,
                    (true, ResetTarget::Both) => Self::QUEUE_RESET_BOTH_INIT_TYPE,
                    (true, ResetTarget::Stream) => Self::QUEUE_RESET_STREAM_INIT_TYPE,
                    (true, ResetTarget::Control) => Self::QUEUE_RESET_CONTROL_INIT_TYPE,
                };
                encoder.encode(&reset_type);
                encoder.encode(dest_queue_id);
                if let Some(acceptor_id) = dest_acceptor_id {
                    encoder.encode(acceptor_id);
                }
                encoder.encode(binding_id);
                encoder.encode(error_code);
            }
            Self::QueueFree {
                free_request_id,
                smallest_queue_id,
            } => {
                encoder.encode(&Self::QUEUE_FREE_TYPE);
                encoder.encode(free_request_id);
                encoder.encode(smallest_queue_id);
            }
            Self::Ack {
                dest_sender_id,
                ack_delay,
                largest_acknowledged,
                ack_range,
                ecn_counts,
                is_ack_eliciting,
            } => {
                let tag = if *is_ack_eliciting {
                    Self::ACK_ELICITING_TYPE
                } else {
                    Self::ACK_TYPE
                };
                encoder.encode(&tag);
                encoder.encode(dest_sender_id);
                encoder.encode(ack_delay);
                encoder.encode(largest_acknowledged);
                encoder.encode(ack_range);
                encoder.encode(&ecn_counts.ect_0_count);
                encoder.encode(&ecn_counts.ect_1_count);
                encoder.encode(&ecn_counts.ce_count);
            }
            Self::QueueMsg {
                queue_pair,
                binding_id,
                msg_id,
                stream_offset,
                message_size,
                chunk_size,
                chunk_index,
                is_fin,
                is_wakeup,
                dest_acceptor_id,
                priority,
            } => {
                let tag = Self::QUEUE_MSG_BASE_TYPE
                    + (*is_fin as u8)
                    + ((*is_wakeup as u8) << 1)
                    + ((dest_acceptor_id.is_some() as u8) << 2);
                encoder.encode(&tag);
                encoder.encode(queue_pair);
                if let Some(acceptor_id) = dest_acceptor_id {
                    encoder.encode(acceptor_id);
                    encoder.encode(&priority.as_u8());
                }
                encoder.encode(binding_id);
                encoder.encode(msg_id);
                encoder.encode(stream_offset);
                encoder.encode(message_size);
                encoder.encode(chunk_size);
                encoder.encode(chunk_index);
            }
            Self::Ping => {
                encoder.encode(&Self::PING_TYPE);
            }
        }
    }
}

impl<'a> s2n_codec::DecoderValue<'a> for Header {
    #[inline]
    fn decode(buffer: s2n_codec::DecoderBuffer<'a>) -> s2n_codec::DecoderBufferResult<'a, Self> {
        let (tag, buffer) = buffer.decode::<u8>()?;

        match tag {
            Self::QUEUE_DATA_INIT_NO_FIN_TYPE | Self::QUEUE_DATA_INIT_WITH_FIN_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (dest_acceptor_id, buffer) = buffer.decode::<VarInt>()?;
                let (priority_byte, buffer) = buffer.decode::<u8>()?;
                let priority = crate::credit::Priority::from_u8(priority_byte).ok_or(
                    s2n_codec::DecoderError::InvariantViolation(
                        "credit::Priority value out of range",
                    ),
                )?;
                let (binding_id, buffer) = buffer.decode()?;
                let (offset, buffer) = buffer.decode()?;
                let is_fin = tag == Self::QUEUE_DATA_INIT_WITH_FIN_TYPE;
                Ok((
                    Self::QueueData {
                        queue_pair,
                        binding_id,
                        offset,
                        is_fin,
                        dest_acceptor_id: Some(dest_acceptor_id),
                        priority,
                    },
                    buffer,
                ))
            }
            Self::QUEUE_DATA_NO_FIN_TYPE | Self::QUEUE_DATA_WITH_FIN_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (binding_id, buffer) = buffer.decode()?;
                let (offset, buffer) = buffer.decode()?;
                let is_fin = tag == Self::QUEUE_DATA_WITH_FIN_TYPE;
                Ok((
                    Self::QueueData {
                        queue_pair,
                        binding_id,
                        offset,
                        is_fin,
                        dest_acceptor_id: None,
                        priority: crate::credit::Priority::default(),
                    },
                    buffer,
                ))
            }
            Self::QUEUE_CONTROL_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (binding_id, buffer) = buffer.decode()?;
                Ok((
                    Self::QueueControl {
                        queue_pair,
                        binding_id,
                    },
                    buffer,
                ))
            }
            Self::QUEUE_MAX_DATA_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (binding_id, buffer) = buffer.decode()?;
                let (maximum_data, buffer) = buffer.decode()?;
                Ok((
                    Self::QueueMaxData {
                        queue_pair,
                        binding_id,
                        maximum_data,
                    },
                    buffer,
                ))
            }
            Self::QUEUE_RESET_BOTH_INIT_TYPE
            | Self::QUEUE_RESET_STREAM_INIT_TYPE
            | Self::QUEUE_RESET_CONTROL_INIT_TYPE => {
                let reset_target = match tag {
                    Self::QUEUE_RESET_BOTH_INIT_TYPE => ResetTarget::Both,
                    Self::QUEUE_RESET_STREAM_INIT_TYPE => ResetTarget::Stream,
                    Self::QUEUE_RESET_CONTROL_INIT_TYPE => ResetTarget::Control,
                    _ => unreachable!(),
                };
                let (dest_queue_id, buffer) = buffer.decode()?;
                let (dest_acceptor_id, buffer) = buffer.decode::<VarInt>()?;
                let (binding_id, buffer) = buffer.decode()?;
                let (error_code, buffer) = buffer.decode()?;
                Ok((
                    Self::QueueReset {
                        dest_queue_id,
                        binding_id,
                        reset_target,
                        error_code,
                        dest_acceptor_id: Some(dest_acceptor_id),
                    },
                    buffer,
                ))
            }
            Self::QUEUE_RESET_BOTH_TYPE
            | Self::QUEUE_RESET_STREAM_TYPE
            | Self::QUEUE_RESET_CONTROL_TYPE => {
                let reset_target = match tag {
                    Self::QUEUE_RESET_BOTH_TYPE => ResetTarget::Both,
                    Self::QUEUE_RESET_STREAM_TYPE => ResetTarget::Stream,
                    Self::QUEUE_RESET_CONTROL_TYPE => ResetTarget::Control,
                    _ => unreachable!(),
                };
                let (dest_queue_id, buffer) = buffer.decode()?;
                let (binding_id, buffer) = buffer.decode()?;
                let (error_code, buffer) = buffer.decode()?;
                Ok((
                    Self::QueueReset {
                        dest_queue_id,
                        binding_id,
                        reset_target,
                        error_code,
                        dest_acceptor_id: None,
                    },
                    buffer,
                ))
            }
            Self::QUEUE_FREE_TYPE => {
                let (free_request_id, buffer) = buffer.decode()?;
                let (smallest_queue_id, buffer) = buffer.decode()?;
                Ok((
                    Self::QueueFree {
                        free_request_id,
                        smallest_queue_id,
                    },
                    buffer,
                ))
            }
            Self::ACK_TYPE | Self::ACK_ELICITING_TYPE => {
                let (dest_sender_id, buffer) = buffer.decode()?;
                let (ack_delay, buffer) = buffer.decode()?;
                let (largest_acknowledged, buffer) = buffer.decode()?;
                let (ack_range, buffer) = buffer.decode()?;
                let (ect_0_count, buffer) = buffer.decode()?;
                let (ect_1_count, buffer) = buffer.decode()?;
                let (ce_count, buffer) = buffer.decode()?;
                let is_ack_eliciting = tag == Self::ACK_ELICITING_TYPE;
                Ok((
                    Self::Ack {
                        dest_sender_id,
                        ack_delay,
                        largest_acknowledged,
                        ack_range,
                        ecn_counts: EcnCounts {
                            ect_0_count,
                            ect_1_count,
                            ce_count,
                        },
                        is_ack_eliciting,
                    },
                    buffer,
                ))
            }
            tag @ Self::QUEUE_MSG_BASE_TYPE..=Self::QUEUE_MSG_MAX_TYPE => {
                let flags = tag - Self::QUEUE_MSG_BASE_TYPE;
                let is_fin = flags & 1 != 0;
                let is_wakeup = flags & 2 != 0;
                let has_init = flags & 4 != 0;
                let (queue_pair, buffer) = buffer.decode()?;
                let (dest_acceptor_id, priority, buffer) = if has_init {
                    let (id, buf) = buffer.decode::<VarInt>()?;
                    let (priority_byte, buf) = buf.decode::<u8>()?;
                    let priority = crate::credit::Priority::from_u8(priority_byte).ok_or(
                        s2n_codec::DecoderError::InvariantViolation(
                            "credit::Priority value out of range",
                        ),
                    )?;
                    (Some(id), priority, buf)
                } else {
                    (None, crate::credit::Priority::default(), buffer)
                };
                let (binding_id, buffer) = buffer.decode()?;
                let (msg_id, buffer) = buffer.decode()?;
                let (stream_offset, buffer) = buffer.decode()?;
                let (message_size, buffer) = buffer.decode()?;
                let (chunk_size, buffer) = buffer.decode()?;
                let (chunk_index, buffer) = buffer.decode()?;
                Ok((
                    Self::QueueMsg {
                        queue_pair,
                        binding_id,
                        msg_id,
                        stream_offset,
                        message_size,
                        chunk_size,
                        chunk_index,
                        is_fin,
                        is_wakeup,
                        dest_acceptor_id,
                        priority,
                    },
                    buffer,
                ))
            }
            Self::PING_TYPE => Ok((Self::Ping, buffer)),
            _ => {
                decoder_invariant!(false, "unknown frame header type");
                Err(s2n_codec::DecoderError::InvariantViolation(
                    "unknown frame header type",
                ))
            }
        }
    }
}

/// A frame submitted by application-level components (Writers, control message senders).
///
/// This is the universal unit of work in the frame aggregation architecture. The transport
/// layer aggregates multiple Frames into single encrypted packets to amortize per-packet costs.
///
/// The same Frame moves through different intrusive queues during its lifecycle (submission,
/// wheel, peer context, packet_number_map, completion) without boxing/unboxing.
pub struct Frame {
    /// Routing metadata for this frame
    pub header: Header,
    /// Payload data (stream bytes for QueueData, control frame bytes for QueueControl,
    /// ACK frames for Control, empty for resets)
    pub payload: ByteVec,
    /// Path secret entry identifying the destination peer.
    ///
    /// Used by the wheel to group frames by peer (Arc pointer comparison) and by the
    /// Peer Context to obtain crypto state for encryption.
    pub path_secret_entry: Arc<PathSecretEntry>,
    /// Completion notification sender. When the frame is acknowledged (or fails), the
    /// completion fires to notify the Writer so it can free inflight budget.
    ///
    /// Also provides cancellation: `completion.should_transmit()` returns false when the
    /// Writer has been dropped or the stream cancelled, causing the transport to skip
    /// this frame rather than transmitting it.
    pub completion: Option<CompletionSender>,
    /// Current transmission status (updated by the pipeline on ACK or loss)
    pub status: TransmissionStatus,
    /// Remaining transmission attempts. Decremented on each retransmission.
    /// When zero, the frame completes with failure instead of being retransmitted.
    pub ttl: u16,
    /// Time at which the application enqueued this frame into the pipeline.
    ///
    /// Set by application-originated frames (data, FIN, MAX_DATA) at construction
    /// time to enable end-to-end sojourn time measurement from submission through
    /// final disposition (ACK, cancellation, peer dead, etc.).
    ///
    /// `None` for transport-internal frames (resets, ACK frames, free frames) that
    /// do not require sojourn tracking.
    pub enqueued_at: Option<precision::Timestamp>,
    /// Bytes of credit-pool budget this frame is currently holding from the
    /// endpoint's send credit pool.
    ///
    /// The producer (currently the Writer) acquires from `Endpoint::send_credit_pool`
    /// before submitting the frame and stores the granted amount here. The pipeline
    /// is responsible for releasing those credits exactly once over the frame's
    /// lifetime — once a credit is released, this field is zeroed so subsequent
    /// disposal cannot double-release.
    ///
    /// Release sites:
    ///
    /// * The assembler, immediately before inserting the frame's packet into the
    ///   inflight map (the "frame is admitted to the wire" point) — `flow_credits`
    ///   is summed across the whole packet and released in one call.
    /// * The cancelled drain task, for frames the assembler routed to the cancelled
    ///   channel because `should_transmit()` returned false (writer dropped, stream
    ///   reset) — these frames never reach the inflight map, so the drain owns
    ///   the release.
    ///
    /// Frames with no associated credit (control frames, ACKs, resets, etc.) leave
    /// this at 0 and the release sites no-op for them.
    pub flow_credits: u64,
}

impl Frame {
    #[inline]
    pub fn priority(&self) -> Priority {
        self.header.priority()
    }

    #[inline]
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Returns true if this frame should still be transmitted.
    ///
    /// Delegates to the completion sender's cancellation flag. Returns true if there's no
    /// completion sender (best-effort frames are always transmittable).
    #[inline]
    pub fn should_transmit(&self) -> bool {
        self.completion.as_ref().is_none_or(|c| c.should_transmit())
    }
}

impl ByteCost for Frame {
    /// Returns the total wire cost of this frame: payload bytes plus the header metadata
    /// (type tag, routing fields, and optional payload-length varint).
    ///
    /// Used by `send::Context::pending_bytes` to track in-queue load without traversal,
    /// and by the pick-two load balancer via `publish_next_transmission_time`.
    #[inline]
    fn byte_cost(&self) -> u64 {
        let payload_len = self.payload.len();
        (payload_len + self.header.metadata_len(payload_len)) as u64
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("header", &self.header)
            .field("payload_len", &self.payload.len())
            .field("peer_data_addrs", self.path_secret_entry.peer_data_addrs())
            .field("ttl", &self.ttl)
            .field("enqueued_at", &self.enqueued_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_data() {
        let entry = PathSecretEntry::builder("127.0.0.1:8080".parse().unwrap()).build();
        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"hello"));

        let frame = Frame {
            header: Header::QueueData {
                queue_pair: QueuePair {
                    source_queue_id: VarInt::from_u8(1),
                    dest_queue_id: VarInt::from_u8(2),
                },
                binding_id: VarInt::from_u8(42),
                offset: VarInt::ZERO,
                is_fin: false,
                dest_acceptor_id: None,
                priority: crate::credit::Priority::default(),
            },
            payload,
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: None,
            flow_credits: 0,
        };

        assert_eq!(frame.payload_len(), 5);
        assert_eq!(frame.ttl, DEFAULT_TTL);
        assert!(frame.should_transmit());
        assert_eq!(frame.priority(), Priority::QueueData);
    }

    #[test]
    fn queue_reset() {
        let entry = PathSecretEntry::builder("10.0.0.1:9000".parse().unwrap()).build();

        let frame = Frame {
            header: Header::QueueReset {
                dest_queue_id: VarInt::from_u8(3),
                binding_id: VarInt::from_u8(42),
                reset_target: ResetTarget::Both,
                error_code: VarInt::from_u8(1),
                dest_acceptor_id: None,
            },
            payload: ByteVec::new(),
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: None,
            flow_credits: 0,
        };

        assert_eq!(frame.priority(), Priority::QueueReset);
        assert_eq!(frame.payload_len(), 0);
        assert_eq!(
            *frame.path_secret_entry.peer(),
            "10.0.0.1:9000".parse::<std::net::SocketAddr>().unwrap()
        );
    }

    #[test]
    fn queue_msg_priority() {
        let header = Header::QueueMsg {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(10),
            msg_id: VarInt::from_u8(0),
            stream_offset: VarInt::ZERO,
            message_size: VarInt::new(65536).unwrap(),
            chunk_size: VarInt::new(8192).unwrap(),
            chunk_index: VarInt::ZERO,
            is_fin: false,
            is_wakeup: true,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        };
        assert_eq!(header.priority(), Priority::QueueData);
        assert!(header.has_payload_length());
        assert!(header.is_ack_eliciting());

        let header_init = Header::QueueMsg {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(10),
            msg_id: VarInt::from_u8(0),
            stream_offset: VarInt::ZERO,
            message_size: VarInt::new(65536).unwrap(),
            chunk_size: VarInt::new(8192).unwrap(),
            chunk_index: VarInt::ZERO,
            is_fin: false,
            is_wakeup: true,
            dest_acceptor_id: Some(VarInt::from_u8(99)),
            priority: crate::credit::Priority::default(),
        };
        assert_eq!(header_init.priority(), Priority::QueueInit);
    }

    #[test]
    fn init_priority_roundtrip_all_levels() {
        let qp = QueuePair {
            source_queue_id: VarInt::from_u8(1),
            dest_queue_id: VarInt::from_u8(2),
        };
        for priority in crate::credit::Priority::ALL {
            roundtrip(Header::QueueData {
                queue_pair: qp,
                binding_id: VarInt::from_u8(10),
                offset: VarInt::ZERO,
                is_fin: false,
                dest_acceptor_id: Some(VarInt::from_u8(99)),
                priority,
            });
            roundtrip(Header::QueueMsg {
                queue_pair: qp,
                binding_id: VarInt::from_u8(10),
                msg_id: VarInt::from_u8(0),
                stream_offset: VarInt::ZERO,
                message_size: VarInt::from_u8(100),
                chunk_size: VarInt::from_u8(100),
                chunk_index: VarInt::ZERO,
                is_fin: false,
                is_wakeup: false,
                dest_acceptor_id: Some(VarInt::from_u8(99)),
                priority,
            });
        }
    }

    #[test]
    fn init_priority_rejects_out_of_range() {
        use s2n_codec::{DecoderBuffer, EncoderBuffer};

        // Use values that all fit in 1-byte VarInts (< 64) so the priority byte's
        // index in the buffer is predictable: tag(1) + src_qid(1) + dst_qid(1) +
        // acceptor_id(1) = 4, priority byte at index 4.
        let header = Header::QueueData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(10),
            offset: VarInt::ZERO,
            is_fin: false,
            dest_acceptor_id: Some(VarInt::from_u8(7)),
            priority: crate::credit::Priority::Highest,
        };
        let mut buf = vec![0u8; header.encoding_size()];
        let mut encoder = EncoderBuffer::new(&mut buf);
        header.encode(&mut encoder);
        assert_eq!(buf[4], 0, "expected priority byte to be Highest=0");
        buf[4] = 8;

        let decoder = DecoderBuffer::new(&buf);
        let result = decoder.decode::<Header>();
        assert!(
            result.is_err(),
            "expected decoder to reject out-of-range priority"
        );
    }

    fn roundtrip(header: Header) {
        use s2n_codec::{DecoderBuffer, EncoderBuffer};

        let header = header.canonicalize_for_wire();
        let mut buf = vec![0u8; header.encoding_size()];
        let mut encoder = EncoderBuffer::new(&mut buf);
        header.encode(&mut encoder);

        let decoder = DecoderBuffer::new(&buf);
        let (decoded, remaining) = decoder.decode::<Header>().unwrap();
        assert!(remaining.is_empty(), "trailing bytes after decode");
        assert_eq!(header, decoded);
    }

    #[test]
    fn queue_msg_roundtrip_all_variants() {
        let qp = QueuePair {
            source_queue_id: VarInt::from_u8(5),
            dest_queue_id: VarInt::from_u8(7),
        };
        let binding_id = VarInt::from_u8(42);
        let msg_id = VarInt::from_u8(3);
        let message_size = VarInt::new(8192).unwrap();
        let chunk_size = VarInt::new(8192).unwrap();
        let chunk_index = VarInt::from_u8(0);
        let acceptor_id = VarInt::from_u8(200);

        for is_fin in [false, true] {
            for is_wakeup in [false, true] {
                for dest_acceptor_id in [None, Some(acceptor_id)] {
                    for priority in crate::credit::Priority::ALL {
                        roundtrip(Header::QueueMsg {
                            queue_pair: qp,
                            binding_id,
                            msg_id,
                            stream_offset: VarInt::ZERO,
                            message_size,
                            chunk_size,
                            chunk_index,
                            is_fin,
                            is_wakeup,
                            dest_acceptor_id,
                            priority,
                        });
                    }
                }
            }
        }
    }

    #[test]
    fn queue_msg_type_tags() {
        use s2n_codec::EncoderBuffer;

        let qp = QueuePair {
            source_queue_id: VarInt::from_u8(1),
            dest_queue_id: VarInt::from_u8(2),
        };

        let cases: Vec<(bool, bool, Option<VarInt>, u8)> = vec![
            (false, false, None, 16),
            (true, false, None, 17),
            (false, true, None, 18),
            (true, true, None, 19),
            (false, false, Some(VarInt::from_u8(1)), 20),
            (true, false, Some(VarInt::from_u8(1)), 21),
            (false, true, Some(VarInt::from_u8(1)), 22),
            (true, true, Some(VarInt::from_u8(1)), 23),
        ];

        for (is_fin, is_wakeup, dest_acceptor_id, expected_tag) in cases {
            let header = Header::QueueMsg {
                queue_pair: qp,
                binding_id: VarInt::from_u8(10),
                msg_id: VarInt::from_u8(0),
                stream_offset: VarInt::ZERO,
                message_size: VarInt::from_u8(100),
                chunk_size: VarInt::from_u8(100),
                chunk_index: VarInt::ZERO,
                is_fin,
                is_wakeup,
                dest_acceptor_id,
                priority: crate::credit::Priority::default(),
            };

            let mut buf = vec![0u8; header.encoding_size()];
            let mut encoder = EncoderBuffer::new(&mut buf);
            header.encode(&mut encoder);

            assert_eq!(
                buf[0],
                expected_tag,
                "tag mismatch for is_fin={is_fin}, is_wakeup={is_wakeup}, init={}",
                dest_acceptor_id.is_some()
            );
        }
    }

    #[test]
    fn queue_msg_encoding_snapshot() {
        use s2n_codec::EncoderBuffer;

        let header = Header::QueueMsg {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(5),
                dest_queue_id: VarInt::from_u8(7),
            },
            binding_id: VarInt::from_u8(42),
            msg_id: VarInt::from_u8(3),
            stream_offset: VarInt::new(32768).unwrap(),
            message_size: VarInt::new(65536).unwrap(),
            chunk_size: VarInt::new(8192).unwrap(),
            chunk_index: VarInt::from_u8(1),
            is_fin: false,
            is_wakeup: true,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        };

        let mut buf = vec![0u8; header.encoding_size()];
        let mut encoder = EncoderBuffer::new(&mut buf);
        header.encode(&mut encoder);

        // tag=18 (base 16 + wakeup bit 1<<1), queue_pair(5,7), binding=42, msg_id=3,
        // stream_offset=32768 (4-byte varint), message_size=65536 (4-byte varint),
        // chunk_size=8192 (2-byte varint), chunk_index=1 (1 byte)
        assert_eq!(buf[0], 18);
        assert_eq!(header.encoding_size(), 1 + 1 + 1 + 1 + 1 + 4 + 4 + 2 + 1);
    }

    #[test]
    fn queue_msg_encoding_snapshot_with_init() {
        use s2n_codec::EncoderBuffer;

        let header = Header::QueueMsg {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(5),
                dest_queue_id: VarInt::from_u8(7),
            },
            binding_id: VarInt::from_u8(42),
            msg_id: VarInt::from_u8(0),
            stream_offset: VarInt::ZERO,
            message_size: VarInt::new(1048576).unwrap(),
            chunk_size: VarInt::new(8192).unwrap(),
            chunk_index: VarInt::ZERO,
            is_fin: true,
            is_wakeup: true,
            dest_acceptor_id: Some(VarInt::from_u8(99)),
            priority: crate::credit::Priority::default(),
        };

        let mut buf = vec![0u8; header.encoding_size()];
        let mut encoder = EncoderBuffer::new(&mut buf);
        header.encode(&mut encoder);

        // tag=23 (base 16 + fin 1 + wakeup 2 + init 4)
        assert_eq!(buf[0], 23);
    }

    #[test]
    fn queue_msg_large_varints_roundtrip() {
        let header = Header::QueueMsg {
            queue_pair: QueuePair {
                source_queue_id: VarInt::new(1_000_000).unwrap(),
                dest_queue_id: VarInt::new(2_000_000).unwrap(),
            },
            binding_id: VarInt::new(500_000).unwrap(),
            msg_id: VarInt::new(999_999).unwrap(),
            stream_offset: VarInt::new(50_000_000).unwrap(),
            message_size: VarInt::new(6_000_000).unwrap(),
            chunk_size: VarInt::new(8192).unwrap(),
            chunk_index: VarInt::new(200).unwrap(),
            is_fin: false,
            is_wakeup: true,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        };
        roundtrip(header);
    }

    #[test]
    fn queue_msg_bolero_roundtrip() {
        bolero::check!().with_type::<Header>().for_each(|header| {
            roundtrip(*header);
        });
    }

    /// Computes the worst-case per-packet overhead (everything except the frame payload bytes).
    ///
    /// Components:
    /// - packet tag (1 byte)
    /// - credentials: Id (16 bytes) + KeyId (VarInt worst-case)
    /// - wire version (1 byte)
    /// - source_control_port (2 bytes)
    /// - packet_number (VarInt worst-case)
    /// - routing_info: type tag (1) + source_sender_id (VarInt worst-case)
    /// - payload_len (VarInt worst-case)
    /// - header_len (VarInt encoding of frame metadata size)
    /// - frame metadata: header encoding + payload_len VarInt
    /// - crypto auth tag (16 bytes)
    fn compute_worst_case_overhead(header: Header) -> usize {
        use crate::{
            credentials::{Credentials, Id},
            packet::{datagram::RoutingInfo, wire_version::WireVersion},
        };

        let packet_tag_size = 1usize;
        let credentials = Credentials {
            id: Id::default(),
            key_id: VarInt::MAX,
        };
        let credentials_size = credentials.encoding_size();
        let wire_version_size = WireVersion::ZERO.encoding_size();
        let source_control_port_size = 0u16.encoding_size();
        let packet_number_size = VarInt::MAX.encoding_size();
        let routing_info = RoutingInfo::SenderId {
            source_sender_id: VarInt::MAX,
        };
        let routing_info_size = routing_info.encoding_size();
        let payload_len_size = VarInt::MAX.encoding_size();
        let crypto_tag_size = 16usize;

        // Frame metadata: header encoding + payload_len VarInt (worst-case payload len)
        let frame_metadata_size = header.encoding_size() + VarInt::MAX.encoding_size();
        // header_len VarInt encodes the frame_metadata_size value
        let header_len_size = VarInt::new(frame_metadata_size as u64)
            .unwrap()
            .encoding_size();

        packet_tag_size
            + credentials_size
            + wire_version_size
            + source_control_port_size
            + packet_number_size
            + routing_info_size
            + payload_len_size
            + header_len_size
            + frame_metadata_size
            + crypto_tag_size
    }

    #[test]
    fn max_queue_data_header_overhead_matches_worst_case() {
        // The constant covers the non-init variant (no dest_acceptor_id) since init frames
        // use Priority::QueueInit and go through a different path.
        let worst_case_header = Header::QueueData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::MAX,
                dest_queue_id: VarInt::MAX,
            },
            binding_id: VarInt::MAX,
            offset: VarInt::MAX,
            is_fin: true,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        };

        let computed = compute_worst_case_overhead(worst_case_header);
        assert_eq!(
            computed as u16, MAX_QUEUE_DATA_HEADER_OVERHEAD,
            "MAX_QUEUE_DATA_HEADER_OVERHEAD ({MAX_QUEUE_DATA_HEADER_OVERHEAD}) does not match \
             computed worst-case ({computed})"
        );
    }

    #[test]
    fn max_queue_msg_header_overhead_matches_worst_case() {
        // Non-init variant (no dest_acceptor_id) — matches QueueData pattern where
        // the init overhead is only relevant for the first frame.
        let worst_case_header = Header::QueueMsg {
            queue_pair: QueuePair {
                source_queue_id: VarInt::MAX,
                dest_queue_id: VarInt::MAX,
            },
            binding_id: VarInt::MAX,
            msg_id: VarInt::MAX,
            stream_offset: VarInt::MAX,
            message_size: VarInt::MAX,
            chunk_size: VarInt::MAX,
            chunk_index: VarInt::MAX,
            is_fin: true,
            is_wakeup: true,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        };

        let computed = compute_worst_case_overhead(worst_case_header);
        assert_eq!(
            computed as u16, MAX_QUEUE_MSG_HEADER_OVERHEAD,
            "MAX_QUEUE_MSG_HEADER_OVERHEAD ({MAX_QUEUE_MSG_HEADER_OVERHEAD}) does not match \
             computed worst-case ({computed})"
        );
    }
}

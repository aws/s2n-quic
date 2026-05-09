// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Packet assembly: packs pending frames into MTU-sized encrypted packets.
//!
//! Called by the Dispatcher when a send::Context fires from the local wheel.
//! Drains frames from the pending queue, packs them into segments respecting
//! MTU and CCA window constraints, encrypts, and registers in the inflight map.

use crate::{
    clock::precision,
    crypto::seal,
    intrusive_queue::Queue,
    packet::{self, datagram::RoutingInfo},
    socket::pool,
    stream3::{
        endpoint::{inflight, send::Context},
        frame::Frame,
    },
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{buffer, time::Clock, varint::VarInt};

/// Attempt to assemble pending frames into a full GSO datagram of encrypted packets.
///
/// Returns None if the CCA window is full, no transmittable frames exist, or pool
/// allocation fails. The caller is responsible for re-registering the context in the
/// timer wheel if frames remain after assembly.
///
/// `header_buf` is a caller-provided reusable allocation for encoding per-frame metadata
/// into the application header region. It is cleared before use and after return.
///
/// Cancelled frames (where `should_transmit()` returns false) are sent to `cancelled`
/// for completion notification.
pub(crate) fn assemble<Clk>(
    context: &mut Context,
    clock: &Clk,
    source_sender_id: VarInt,
    source_control_port: u16,
    pool: &pool::Pool,
    header_buf: &mut Vec<u8>,
    cancelled: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
) -> Option<pool::descriptor::Segments>
where
    Clk: precision::Clock + Clock + ?Sized,
{
    let available_window = context
        .cca
        .congestion_window()
        .saturating_sub(context.cca.bytes_in_flight());

    if available_window == 0 {
        return None;
    }

    let mtu = context.path_secret_entry.max_datagram_size();
    let now = clock.now();
    let time_sent = clock.get_time();

    let unfilled = pool.alloc()?;

    let mut segment_size: u16 = 0;
    let mut segments_written: u32 = 0;

    let result = unfilled.fill_with(|addr, cmsg, mut payload| {
        addr.set(context.path_secret_entry.data_addr().into());

        let mut offset: usize = 0;
        let mut watermark: usize = 0;

        loop {
            // Check if we have buffer capacity for another segment
            if offset + mtu as usize > payload.len() {
                break;
            }

            // Drain cancelled frames before collecting transmittable ones
            let mut cancelled_queue = Queue::new();
            let mut packet_frames = Queue::new();
            let mut total_payload_bytes: usize = 0;

            while let Some(frame) = context.pending.pop_front() {
                if !frame.should_transmit() {
                    cancelled_queue.push_back(frame);
                    continue;
                }

                let frame_payload = frame.payload_len();

                // Check if adding this frame would exceed MTU.
                // The actual segment size depends on packet header + frame headers + payloads + tag,
                // but we use payload bytes as the primary packing heuristic since per-frame header
                // overhead is small relative to MTU.
                if !packet_frames.is_empty() && total_payload_bytes + frame_payload > mtu as usize {
                    context.pending.push_front(frame);
                    break;
                }

                total_payload_bytes += frame_payload;
                packet_frames.push_back(frame);

                if total_payload_bytes >= mtu as usize {
                    break;
                }
            }

            // Send cancelled frames to the completion channel
            if !cancelled_queue.is_empty() {
                let _ = cancelled.send(cancelled_queue);
            }

            if packet_frames.is_empty() {
                break;
            }

            // Assign packet number
            let packet_number = context.next_packet_number;
            context.next_packet_number += 1;

            // Zero padding between segments for GSO alignment
            if offset > watermark {
                payload[watermark..offset].fill(0);
            }

            // Encode this segment
            header_buf.clear();
            let encoded_len = encode_segment(
                &mut payload[offset..],
                source_control_port,
                source_sender_id,
                packet_number,
                &context.sealer,
                &context.credentials,
                &mut context.flow_attempt_id_counter,
                &packet_frames,
                header_buf,
            );

            watermark = offset + encoded_len;

            // First segment establishes GSO segment size
            if segment_size == 0 {
                segment_size = encoded_len as u16;
            }

            // Register in inflight map
            let has_more_app_data = context.has_pending();
            let cc_info = context.cca.on_packet_sent(
                time_sent,
                encoded_len as u16,
                has_more_app_data,
                &context.rtt_estimator,
            );
            let tx_info = inflight::TransmissionInfo {
                cc_info,
                time_sent,
                sent_bytes: encoded_len as u16,
            };
            let pn = s2n_quic_core::packet::number::PacketNumberSpace::Initial
                .new_packet_number(packet_number);
            context
                .inflight
                .insert(pn, inflight::Packet::new(packet_frames, tx_info));

            segments_written += 1;

            // Advance to next segment boundary
            offset += segment_size as usize;

            // Undersized segment must be last (GSO constraint)
            if (encoded_len as u16) < segment_size {
                break;
            }
        }

        if segments_written > 0 && segment_size > 0 {
            cmsg.set_segment_len(segment_size);
        }

        <Result<_, core::convert::Infallible>>::Ok(watermark)
    });

    let segments = result.expect("fill_with closure is infallible");

    if segments_written == 0 {
        return None;
    }

    // Update PTO
    context.pto.on_packet_sent(now);

    header_buf.clear();

    Some(pool::descriptor::Segments::new(
        segments.take_filled(),
        segment_size,
    ))
}

/// Encode a single segment containing one or more frames.
///
/// Wire layout:
///   [packet-level header: tag, credentials, wire_version, source_control_port, pn, SenderId routing]
///   [header_len varint][frame metadata: Header + payload_len per frame...]
///   [payload_len varint][frame payloads concatenated...]
///   [auth tag: 16 bytes]
///
/// The packet header through the frame metadata region is cleartext (authenticated as AAD).
/// The payload region is encrypted in place.
fn encode_segment<S: seal::Application>(
    buf: &mut [u8],
    source_control_port: u16,
    source_sender_id: VarInt,
    packet_number: VarInt,
    sealer: &S,
    credentials: &crate::credentials::Credentials,
    flow_attempt_id: &mut VarInt,
    frames: &Queue<Frame>,
    header_buf: &mut Vec<u8>,
) -> usize {
    let routing_info = RoutingInfo::SenderId { source_sender_id };

    // Build the application header: per-frame metadata entries
    header_buf.clear();
    let mut total_payload_len: usize = 0;

    for frame in frames.iter() {
        // Stamp attempt_id for FlowInit if needed
        let header = stamp_attempt_id(&frame.header, flow_attempt_id);

        // Encode frame header + payload_len into a stack buffer, then append
        let payload_len = VarInt::try_from(frame.payload_len() as u64).unwrap_or(VarInt::ZERO);
        let entry_size = header.encoding_size() + payload_len.encoding_size();
        let start = header_buf.len();
        header_buf.resize(start + entry_size, 0);
        let mut enc = EncoderBuffer::new(&mut header_buf[start..]);
        enc.encode(&header);
        enc.encode(&payload_len);
        debug_assert_eq!(enc.len(), entry_size);

        total_payload_len += frame.payload_len();
    }

    let header_len = VarInt::try_from(header_buf.len() as u64).unwrap_or(VarInt::ZERO);
    let payload_len_varint = VarInt::try_from(total_payload_len as u64).unwrap_or(VarInt::ZERO);

    // Build a concatenated payload reader over all frame payloads
    let mut payload_reader = FramePayloadReader::new(frames);

    let mut header_reader = &header_buf[..];

    datagram::encoder::encode(
        EncoderBuffer::new(buf),
        source_control_port,
        routing_info,
        Some(packet_number),
        header_len,
        &mut header_reader,
        payload_len_varint,
        &mut payload_reader,
        sealer,
        credentials,
    )
}

/// Produce a Header with attempt_id stamped for FlowInit frames.
fn stamp_attempt_id(
    header: &crate::stream3::frame::Header,
    flow_attempt_id: &mut VarInt,
) -> crate::stream3::frame::Header {
    use crate::stream3::frame::Header;
    match header {
        Header::FlowInit {
            source_queue_id,
            dest_acceptor_id,
            attempt_id,
            stream_id,
            is_fin,
        } => {
            let attempt_id = if *attempt_id == VarInt::MAX {
                let id = *flow_attempt_id;
                *flow_attempt_id += 1;
                id
            } else {
                *attempt_id
            };
            Header::FlowInit {
                source_queue_id: *source_queue_id,
                dest_acceptor_id: *dest_acceptor_id,
                attempt_id,
                stream_id: *stream_id,
                is_fin: *is_fin,
            }
        }
        other => *other,
    }
}

/// A Storage reader that concatenates payloads from multiple frames.
///
/// The encoder calls `partial_copy_into` to drain payload bytes into the packet buffer.
/// This implementation iterates through each frame's ByteVec payload in order.
struct FramePayloadReader {
    /// Concatenated payload from all frames, built once at construction.
    inner: crate::byte_vec::ByteVec,
}

impl FramePayloadReader {
    fn new(frames: &Queue<Frame>) -> Self {
        let mut inner = crate::byte_vec::ByteVec::new();
        for frame in frames.iter() {
            if frame.payload_len() > 0 {
                inner.append(&mut frame.payload.clone());
            }
        }
        Self { inner }
    }
}

impl buffer::reader::Storage for FramePayloadReader {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.inner.len()
    }

    #[inline]
    fn read_chunk(
        &mut self,
        watermark: usize,
    ) -> Result<buffer::reader::storage::Chunk<'_>, Self::Error> {
        self.inner.read_chunk(watermark)
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<buffer::reader::storage::Chunk<'_>, Self::Error>
    where
        Dest: buffer::writer::Storage + ?Sized,
    {
        self.inner.partial_copy_into(dest)
    }
}

use crate::packet::datagram;

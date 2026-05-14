// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Packet assembly: packs pending frames into MTU-sized encrypted packets.
//!
//! Called by the Dispatcher when a send::Context fires from the local wheel.
//! Drains frames from the pending queue, packs them into segments respecting
//! MTU and CCA window constraints, encrypts, and registers in the inflight map.

use crate::{
    clock::precision,
    credentials::Credentials,
    crypto::seal,
    intrusive_queue::{self, Queue},
    msg::segment,
    packet::{
        datagram::{self, RoutingInfo},
        WireVersion,
    },
    socket::{
        channel::UnboundedSender,
        pool::{self, descriptor::Segments},
    },
    stream3::{
        endpoint::{combinator::AssemblerCounters, inflight, msg, send::Context},
        frame::{self, Frame},
    },
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    buffer,
    inet::ExplicitCongestionNotification,
    packet::number::{PacketNumber, PacketNumberSpace},
    varint::VarInt,
};
use s2n_quic_platform::features::Gso;

#[cfg(test)]
mod tests;

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
    gso: &Gso,
    pool: &pool::Pool,
    header_buf: &mut Vec<u8>,
    cancelled: &mut impl UnboundedSender<intrusive_queue::Entry<Frame>>,
    ack_completions: &mut impl UnboundedSender<intrusive_queue::Entry<msg::Sender>>,
    counters: &AssemblerCounters,
) -> Option<Segments>
where
    Clk: precision::Clock + ?Sized,
{
    let mtu = context.path_secret_entry.max_datagram_size();
    let now = clock.now();
    let time_sent = now.into();
    let max_segments = gso.max_segments().min(segment::MAX_COUNT);

    let unfilled = pool.alloc()?;

    let mut segment_size: u16 = 0;
    let mut segments_written: u32 = 0;

    let result = unfilled.fill_with(|addr, cmsg, mut payload| {
        addr.set(context.path_secret_entry.data_addr().into());
        cmsg.set_ecn(ExplicitCongestionNotification::Ect0);

        let mut offset: usize = 0;
        let mut watermark: usize = 0;

        loop {
            if segments_written as usize >= max_segments {
                break;
            }

            // Check if we have buffer capacity for another segment
            if offset + mtu as usize > payload.len() {
                break;
            }

            let max_segment_len = {
                let remaining_total =
                    segment::MAX_TOTAL as usize - offset.min(segment::MAX_TOTAL as usize);
                if segment_size == 0 {
                    remaining_total.min(mtu as usize)
                } else {
                    remaining_total.min(segment_size as usize)
                }
            };

            if max_segment_len == 0 {
                break;
            }

            let mut packet_frames = Queue::new();
            let mut metadata = MetadataEstimate::new(context.flow_attempt_id_counter);
            let mut is_ack_eliciting = false;
            // Number of leading ACK frames in packet_frames (from the direct path).
            // These are stripped before inflight insertion since ACKs are stale on retransmit.
            let mut ack_frame_count: usize = 0;
            // If a probe is encoded in this segment, this records which old inflight
            // entry was turned into a shell so we can link it to the new PN after
            // the segment is registered in the inflight map.
            let mut probe_from_pn: Option<PacketNumber> = None;

            // Phase 1: drain direct ACK submissions (from pending_acks queue).
            // Each entry carries an already-encoded ACK body from recv worker; stamp
            // wire-time ack_delay here. These bypass CWND like Phase 1 frames.
            while let Some(entry) = context.pending_acks.pop_front() {
                let crate::stream3::endpoint::msg::Sender::PendingAck(ref submission) = *entry
                else {
                    unreachable!("pending_acks should only contain PendingAck entries")
                };

                let ack_delay_duration = now.duration_since(submission.largest_recv_time);
                let ack_delay_micros = ack_delay_duration.as_micros() as u64;
                let ack_delay = VarInt::new(ack_delay_micros).unwrap_or(VarInt::from_u32(u32::MAX));

                let header = frame::Header::Ack {
                    dest_sender_id: submission.remote_sender_id,
                    ack_delay,
                    has_ecn: submission.has_ecn,
                };
                let payload_len = submission.body.len();

                let next_metadata = metadata.with_frame_parts(&header, payload_len);
                let estimated_len = next_metadata.estimate_packet_len(
                    source_sender_id,
                    source_control_port,
                    context.next_packet_number,
                    &context.credentials,
                    seal::Application::tag_len(&context.sealer),
                );

                if estimated_len > max_segment_len {
                    context.pending_acks.push_front(entry);
                    break;
                }

                let frame = Frame {
                    header,
                    source_sender_id: submission.local_sender_id,
                    payload: submission.body.clone().into(),
                    path_secret_entry: submission.path_secret_entry.clone(),
                    completion: None,
                    status: Default::default(),
                    ttl: frame::DEFAULT_TTL,
                    transmission_time: None,
                };

                ack_frame_count += 1;
                metadata = next_metadata;
                packet_frames.push_back(frame.into());

                // Return the entry to the recv worker via the completion channel.
                let _ = ack_completions.send(entry);

                if estimated_len == max_segment_len {
                    break;
                }
            }

            // Phase 2: PTO probe assembly.
            //
            // Only entered when a probe is requested and there is no pending data to serve
            // as the probe. If pending data is present, Phase 3 will bypass CWND and act
            // as the probe (RFC 9002 §6.2.4), so no retransmit is needed here.
            //
            // Skipping 4 PNs creates a gap large enough that the peer will immediately
            // ACK the probe (RFC 9000 §13.2.1 / RFC 9002 §6.2.4).
            if context.pto.probe_state.is_requested() && !context.has_pending_data() {
                // No pending data — retransmit from the oldest inflight entry.
                if let Some((old_pn, mut probe_frames)) = context.inflight.take_oldest_for_probe() {
                    // oldest_non_shell_pn only returns entries with non-empty frames.
                    debug_assert!(
                        !probe_frames.is_empty(),
                        "take_oldest_for_probe returned empty frames"
                    );

                    // Compute the post-skip packet number without committing it to
                    // context yet — we only assign it if at least one frame fits.
                    let next_packet_number = context.next_packet_number + 4;

                    let mut probe_metadata = metadata;

                    while let Some(frame) = probe_frames.pop_front() {
                        let next = probe_metadata.with_frame(&frame);
                        let est_len = next.estimate_packet_len(
                            source_sender_id,
                            source_control_port,
                            next_packet_number,
                            &context.credentials,
                            seal::Application::tag_len(&context.sealer),
                        );
                        if est_len <= max_segment_len {
                            probe_metadata = next;
                            is_ack_eliciting = true;
                            packet_frames.push_back(frame);
                        } else {
                            // A frame didn't fit.  If this is the very first probe frame
                            // it is a header-estimate bug — MTUs don't change so it must
                            // fit.  Panic loudly so we can fix the estimate.
                            assert!(
                                is_ack_eliciting,
                                "first probe frame does not fit — header estimate is wrong"
                            );
                            // Remaining frames go back into probe_frames for retransmission.
                            probe_frames.push_front(frame);
                            break;
                        }
                    }

                    // Commit the PN skip; at least one frame fit (asserted above).
                    context.next_packet_number = next_packet_number;
                    metadata = probe_metadata;

                    // Always create the shell link regardless of full or partial fit.
                    probe_from_pn = Some(old_pn);

                    // Any frames that didn't fit are scheduled for retransmission so
                    // they go out in the next TX opportunity without waiting for a loss
                    // declaration or the next PTO firing.
                    for frame in probe_frames {
                        context.push_back_frame(frame);
                    }
                    // probe_state cleared below, after encoding, via on_transmit()
                } else {
                    // Inflight was drained (e.g. by an ACK) between the PTO fire and
                    // assembly — clear the probe request so we don't spin.
                    let _ = context.pto.probe_state.on_transmit();
                }
            }

            // Phase 3: drain pending (data) frames.
            // When a probe is requested and pending data is available, bypass CWND per
            // RFC 9002 §6.2.4. Otherwise only drain when the congestion window allows.
            let can_send_pending = context.has_pending_data()
                && (context.pto.probe_state.is_requested() || context.can_send_pending_frames());

            if can_send_pending {
                while let Some(frame) = context.pop_pending() {
                    if !frame.should_transmit() {
                        let _ = cancelled.send(frame);
                        continue;
                    }

                    let next_metadata = metadata.with_frame(&frame);
                    let estimated_len = next_metadata.estimate_packet_len(
                        source_sender_id,
                        source_control_port,
                        context.next_packet_number,
                        &context.credentials,
                        seal::Application::tag_len(&context.sealer),
                    );

                    if estimated_len > max_segment_len {
                        context.push_front_frame(frame);
                        break;
                    }

                    is_ack_eliciting |= frame.header.is_ack_eliciting();
                    metadata = next_metadata;
                    packet_frames.push_back(frame);

                    if estimated_len == max_segment_len {
                        break;
                    }
                }
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
            let encoded_len = {
                let _guard = counters.encrypt_time.start();
                encode_segment(
                    &mut payload[offset..],
                    source_control_port,
                    source_sender_id,
                    packet_number,
                    &context.sealer,
                    &context.credentials,
                    &mut context.flow_attempt_id_counter,
                    &packet_frames,
                    header_buf,
                )
            };

            debug_assert!(encoded_len <= max_segment_len);

            counters.packet_size.record_value(encoded_len as u64);
            counters
                .tx_frames_per_packet
                .record_value(packet_frames.len() as u64);
            counters
                .tx_payload_size
                .record_value(metadata.payload_len as u64);
            counters.tx_data.add(1);

            if probe_from_pn.is_some() {
                counters.tx_probe.add(1);
            }

            watermark = offset + encoded_len;

            // First segment establishes GSO segment size
            if segment_size == 0 {
                segment_size = encoded_len as u16;
            }

            // Verify the is_ack_eliciting flag matches the actual frame list before
            // using it to gate inflight insertion — a mismatch would silently drop packets.
            debug_assert_eq!(
                is_ack_eliciting,
                packet_frames.iter().any(|f| f.header.is_ack_eliciting()),
                "is_ack_eliciting flag does not match actual frames in packet_frames"
            );

            if is_ack_eliciting {
                // Strip leading ACK frames before inflight insertion — they are stale
                // on retransmit and must not be re-sent as probes.
                for _ in 0..ack_frame_count {
                    let frame = packet_frames
                        .pop_front()
                        .expect("ack_frame_count exceeds packet_frames length");
                    debug_assert!(
                        !frame.header.is_ack_eliciting(),
                        "expected ACK frame during stripping, got a data frame"
                    );
                    drop(frame);
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
                let pn = PacketNumberSpace::Initial.new_packet_number(packet_number);
                context
                    .inflight
                    .insert(pn, inflight::Packet::new(packet_frames, tx_info));

                // If this segment was a probe, link the old shell entry to the new PN.
                if let Some(old_pn) = probe_from_pn {
                    context.inflight.set_probed_to(old_pn, pn);
                }

                // Notify probe state that an ack-eliciting packet was transmitted.
                // This clears `Requested → Idle`; if already Idle (e.g. second segment
                // in this assembly round) the NoOp result is silently ignored.
                let _ = context.pto.probe_state.on_transmit();
                context.invariants();
            }

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
        // Frames may have been drained (e.g. all cancelled); publish the updated
        // pending-byte count so the load-balancer sees the reduced queue.
        context.publish_next_transmission_time(time_sent);
        return None;
    }

    // Update PTO
    context.pto.on_packet_sent(now);

    // Publish updated load estimate: both pending queue and CCA state may have changed.
    context.publish_next_transmission_time(time_sent);

    header_buf.clear();

    Some(Segments::new(segments.take_filled(), segment_size))
}

#[derive(Clone, Copy, Debug)]
struct MetadataEstimate {
    header_len: usize,
    payload_len: usize,
    flow_attempt_id: VarInt,
}

impl MetadataEstimate {
    #[inline]
    fn new(flow_attempt_id: VarInt) -> Self {
        Self {
            header_len: 0,
            payload_len: 0,
            flow_attempt_id,
        }
    }

    #[inline]
    fn with_frame(self, frame: &Frame) -> Self {
        self.with_frame_parts(&frame.header, frame.payload_len())
    }

    #[inline]
    fn with_frame_parts(mut self, header: &frame::Header, payload_len: usize) -> Self {
        let header = stamp_attempt_id(header, &mut self.flow_attempt_id);
        self.header_len += frame_metadata_len(&header, payload_len);
        self.payload_len += payload_len;
        self
    }

    #[inline]
    fn estimate_packet_len(
        &self,
        source_sender_id: VarInt,
        source_control_port: u16,
        packet_number: VarInt,
        credentials: &Credentials,
        crypto_tag_len: usize,
    ) -> usize {
        let header_len = VarInt::new(self.header_len as u64).expect("header length fits in VarInt");
        let payload_len =
            VarInt::new(self.payload_len as u64).expect("payload length fits in VarInt");
        let routing_info = RoutingInfo::SenderId { source_sender_id };

        crate::packet::datagram::Tag::default().encoding_size()
            + credentials.encoding_size()
            + WireVersion::ZERO.encoding_size()
            + source_control_port.encoding_size()
            + packet_number.encoding_size()
            + routing_info.encoding_size()
            + payload_len.encoding_size()
            + if self.header_len > 0 {
                header_len.encoding_size() + self.header_len
            } else {
                0
            }
            + self.payload_len
            + crypto_tag_len
    }
}

/// Encode a single segment containing one or more frames.
///
/// Wire layout:
///   [packet-level header: tag, credentials, wire_version, source_control_port, pn, SenderId routing]
///   [header_len varint][frame metadata: Header + optional payload_len per frame...]
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
    credentials: &Credentials,
    flow_attempt_id: &mut VarInt,
    frames: &Queue<Frame>,
    header_buf: &mut Vec<u8>,
) -> usize {
    let routing_info = RoutingInfo::SenderId { source_sender_id };

    // Build the application header: per-frame metadata entries
    let total_payload_len = encode_frame_metadata(frames, flow_attempt_id, header_buf);

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

fn encode_frame_metadata(
    frames: &Queue<Frame>,
    flow_attempt_id: &mut VarInt,
    header_buf: &mut Vec<u8>,
) -> usize {
    header_buf.clear();
    let mut total_payload_len = 0usize;

    for frame in frames.iter() {
        let header = stamp_attempt_id(&frame.header, flow_attempt_id);
        push_frame_metadata(header_buf, &header, frame.payload_len());

        total_payload_len += frame.payload_len();
    }

    total_payload_len
}

#[inline]
fn frame_metadata_len(header: &frame::Header, payload_len: usize) -> usize {
    header.metadata_len(payload_len)
}

#[inline]
fn debug_assert_payload_length_invariant(payload_len: usize) {
    debug_assert_eq!(
        payload_len, 0,
        "frames without payload_length must have zero payload"
    );
}

#[inline]
fn push_frame_metadata(header_buf: &mut Vec<u8>, header: &frame::Header, payload_len: usize) {
    let entry_size = frame_metadata_len(header, payload_len);
    let start = header_buf.len();
    header_buf.resize(start + entry_size, 0);

    let mut enc = EncoderBuffer::new(&mut header_buf[start..]);
    enc.encode(header);

    if header.has_payload_length() {
        let payload_len = VarInt::try_from(payload_len as u64).unwrap_or(VarInt::ZERO);
        enc.encode(&payload_len);
    } else {
        debug_assert_payload_length_invariant(payload_len);
    }

    debug_assert_eq!(
        enc.len(),
        entry_size,
        "frame metadata encoder length mismatch"
    );
}

/// Produce a Header with attempt_id stamped for FlowInit frames.
fn stamp_attempt_id(header: &frame::Header, flow_attempt_id: &mut VarInt) -> frame::Header {
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

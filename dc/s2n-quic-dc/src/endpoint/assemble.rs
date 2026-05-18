// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Packet assembly: packs pending frames into MTU-sized encrypted packets.
//!
//! Called by the Dispatcher when a send::Context fires from the local wheel.
//! Drains frames from the pending queue, packs them into segments respecting
//! MTU and CCA window constraints, encrypts, and registers in the inflight map.

use crate::{
    credentials::Credentials,
    crypto::seal,
    endpoint::{
        combinator::AssemblerCounters,
        frame::{self, Frame},
        inflight, msg,
        send::{Context, PathInfo},
    },
    intrusive::{self, Queue},
    msg::segment,
    packet::{
        datagram::{self, RoutingInfo},
        WireVersion,
    },
    socket::{
        channel::UnboundedSender,
        pool::{self, descriptor::Segments},
    },
    time::precision,
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
    cancelled: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
    ack_completions: &mut impl UnboundedSender<intrusive::Entry<msg::Sender>>,
    counters: &AssemblerCounters,
) -> Option<Segments>
where
    Clk: precision::Clock + ?Sized,
{
    let now = clock.now();
    let time_sent = now.into();
    let PathInfo {
        max_datagram_size: mtu,
        max_segments,
    } = context.path_info(gso);

    let unfilled = pool.alloc()?;

    let mut segment_size: u16 = 0;
    let mut segments_written: u32 = 0;
    let mut sent_inflight_packet = false;

    let result = unfilled.fill_with(|addr, cmsg, mut payload| {
        addr.set(context.peer_addr.into());
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
            let mut metadata = MetadataEstimate::new();
            let mut is_ack_eliciting = false;
            // Number of leading ACK frames in packet_frames (from the direct path).
            // These are stripped before inflight insertion since ACKs are stale on retransmit.
            let mut ack_frame_count: usize = 0;
            // If a probe is encoded in this segment, this records which old inflight
            // entry was turned into a shell so we can link it to the new PN after
            // the segment is registered in the inflight map.
            let mut probe_from_pn: Option<PacketNumber> = None;

            // When there is no data in the inflight map, we want to obtain an RTT
            // sample by making one ACK-only packet ack-eliciting (PING-style probe).
            // `make_ack_eliciting` is true only for the first probe in a cycle;
            // once a probe is in-flight (`is_pending()=true`) subsequent ACK-only
            // sends are non-ack-eliciting but still update `latest` via
            // `on_non_eliciting_sent` so we have the freshest PN when the ACK arrives.
            let rtt_sample_needed = !context.inflight.has_inflight();
            // Only make the ACK packet ack-eliciting when no probe is already
            // outstanding. This prevents an ACK loop: once the peer responds to
            // our ack-eliciting probe, sampled=true keeps is_pending()=true and
            // suppresses further probing until new data enters the inflight map.
            let make_ack_eliciting = rtt_sample_needed && !context.rtt_tracker.is_pending();

            // Phase 1: drain direct ACK submissions (from pending_acks queue).
            // Each entry carries an already-encoded ACK body from recv worker; stamp
            // wire-time ack_delay here. These bypass CWND like Phase 1 frames.
            while let Some(entry) = context.pending_acks.pop_front() {
                let crate::stream::endpoint::msg::Sender::PendingAck(ref submission) = *entry
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
                    is_ack_eliciting: context.pto.probe_state.is_requested()
                        || make_ack_eliciting,
                };
                let payload_len = submission.body.len();

                let next_metadata = metadata.with_frame_parts(&header, payload_len);
                let estimated_len = next_metadata.estimate_packet_len(
                    source_sender_id,
                    source_control_port,
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

                is_ack_eliciting |= header.is_ack_eliciting();
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
                match assemble_probe(
                    context,
                    &mut metadata,
                    &mut packet_frames,
                    source_sender_id,
                    source_control_port,
                    max_segment_len,
                    cancelled,
                ) {
                    ProbeResult::Assembled { old_pn } => {
                        is_ack_eliciting = true;
                        probe_from_pn = Some(old_pn);
                    }
                    ProbeResult::NothingToProbe => {
                        let _ = context.pto.probe_state.on_transmit();
                    }
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
                    context.sender_idx,
                    packet_number,
                    &context.sealer,
                    &context.credentials,
                    &mut context.flow_attempt_id_counter,
                    &mut packet_frames,
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
                        matches!(frame.header, frame::Header::Ack { .. }),
                        "expected ACK frame during stripping, got a data frame"
                    );
                    drop(frame);
                }

                // Notify probe state that an ack-eliciting packet was transmitted.
                // This clears `Requested → Idle`; if already Idle (e.g. second segment
                // in this assembly round) the NoOp result is silently ignored.
                let _ = context.pto.probe_state.on_transmit();

                // Register in inflight map only if data frames remain after stripping ACKs.
                // An ACK-only packet that was ack-eliciting (PING-style ACK) satisfies the
                // PTO but has nothing to retransmit.
                if !packet_frames.is_empty() {
                    // Data frames are going into the inflight map and will produce RTT
                    // samples via normal ACK processing. The separate ACK-only RTT tracker
                    // is no longer needed.
                    context.rtt_tracker.clear();

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
                    sent_inflight_packet = true;

                    // If this segment was a probe, link the old shell entry to the new PN.
                    if let Some(old_pn) = probe_from_pn {
                        context.inflight.set_probed_to(old_pn, pn);
                    }
                } else if rtt_sample_needed {
                    // ACK-only ack-eliciting packet (our own RTT probe or PTO-triggered).
                    // The tracker update must live here — inside the `if is_ack_eliciting`
                    // block — because `is_ack_eliciting=true` prevents the outer
                    // `else if rtt_sample_needed` branch from ever being reached.
                    if make_ack_eliciting {
                        // This is our own ack-eliciting probe; start a new tracking cycle.
                        context.rtt_tracker.on_sent(packet_number, time_sent);
                    } else {
                        // PTO made the packet ack-eliciting while our own probe was not
                        // requested (sampled=true or stable already in-flight). Update
                        // `latest` so if the peer's ACK covers this PN we get a fresh sample.
                        context.rtt_tracker.on_non_eliciting_sent(packet_number, time_sent);
                    }
                }
                context.invariants();
            } else if rtt_sample_needed {
                // `is_ack_eliciting=false` here. By construction, `is_ack_eliciting=false`
                // implies `make_ack_eliciting=false` (a true `make_ack_eliciting` would have
                // set the ACK header ack-eliciting, making `is_ack_eliciting=true`).
                // Keep `latest` fresh while the in-flight probe waits for an ACK.
                context.rtt_tracker.on_non_eliciting_sent(packet_number, time_sent);
                context.invariants();
            } else {
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
        // load score so the pick-two balancer sees the reduced queue.
        context.publish_sender_load_score(time_sent);
        return None;
    }

    if sent_inflight_packet {
        // Update PTO only when an inflight packet was created.
        context.pto.on_packet_sent(now);
    }

    // Publish updated load score: both pending queue and CCA state may have changed.
    context.publish_sender_load_score(time_sent);

    header_buf.clear();

    Some(Segments::new(segments.take_filled(), segment_size))
}

enum ProbeResult {
    Assembled { old_pn: PacketNumber },
    NothingToProbe,
}

/// Try to assemble a PTO probe from the oldest inflight packet(s).
///
/// Skips packets whose frames have all been cancelled (writer dropped). If a
/// transmittable packet is found, its frames are added to `packet_frames` and the
/// PN is advanced by 4 to create the gap that triggers an immediate ACK.
fn assemble_probe(
    context: &mut Context,
    metadata: &mut MetadataEstimate,
    packet_frames: &mut Queue<Frame>,
    source_sender_id: VarInt,
    source_control_port: u16,
    max_segment_len: usize,
    cancelled: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
) -> ProbeResult {
    while let Some((old_pn, mut probe_frames)) = context.inflight.take_oldest_for_probe() {
        let next_packet_number = context.next_packet_number + 4;
        let mut probe_metadata = *metadata;
        let mut has_frame = false;

        while let Some(frame) = probe_frames.pop_front() {
            if !frame.should_transmit() {
                let _ = cancelled.send(frame);
                continue;
            }

            let next = probe_metadata.with_frame(&frame);
            let est_len = next.estimate_packet_len(
                source_sender_id,
                source_control_port,
                &context.credentials,
                seal::Application::tag_len(&context.sealer),
            );
            if est_len <= max_segment_len {
                probe_metadata = next;
                has_frame = true;
                packet_frames.push_back(frame);
            } else if !has_frame {
                // First probe frame doesn't fit. This is only legitimate when
                // ACK frames already consumed part of the segment budget.
                assert!(
                    metadata.payload_len > 0 || metadata.header_len > 0,
                    "first probe frame does not fit in a clean packet — header estimate is wrong; \
                     est_len={est_len}, max_segment_len={max_segment_len}, \
                     metadata={probe_metadata:?}, \
                     frame_header={:?}, frame_payload_len={}",
                    frame.header,
                    frame.payload_len(),
                );
                probe_frames.push_front(frame);
                context.inflight.restore_probe_frames(old_pn, probe_frames);
                return ProbeResult::NothingToProbe;
            } else {
                probe_frames.push_front(frame);
                break;
            }
        }

        if !has_frame {
            // All frames were cancelled — remove the now-empty shell that has no
            // probed_to link, otherwise it violates the inflight invariant.
            // We must also inform the CCA so it releases the bytes from `bytes_in_flight`,
            // otherwise the window is permanently inflated and the context becomes
            // un-schedulable.
            if let Some(packet) = context.inflight.remove(old_pn) {
                if let Some(tx_info) = packet.transmission_info {
                    context.cca.on_packet_discarded(tx_info.sent_bytes as usize);
                }
            }
            continue;
        }

        context.next_packet_number = next_packet_number;
        *metadata = probe_metadata;

        for frame in probe_frames {
            context.push_back_frame(frame);
        }

        return ProbeResult::Assembled { old_pn };
    }

    ProbeResult::NothingToProbe
}

#[derive(Clone, Copy, Debug)]
struct MetadataEstimate {
    header_len: usize,
    payload_len: usize,
}

impl MetadataEstimate {
    #[inline]
    fn new() -> Self {
        Self {
            header_len: 0,
            payload_len: 0,
        }
    }

    #[inline]
    fn with_frame(self, frame: &Frame) -> Self {
        self.with_frame_parts(&frame.header, frame.payload_len())
    }

    #[inline]
    fn with_frame_parts(mut self, header: &frame::Header, payload_len: usize) -> Self {
        self.header_len += frame_metadata_len(header, payload_len);
        self.payload_len += payload_len;
        self
    }

    #[inline]
    fn estimate_packet_len(
        &self,
        source_sender_id: VarInt,
        source_control_port: u16,
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
            + VarInt::MAX.encoding_size()
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
    sender_idx: usize,
    packet_number: VarInt,
    sealer: &S,
    credentials: &Credentials,
    flow_attempt_id: &mut VarInt,
    frames: &mut Queue<Frame>,
    header_buf: &mut Vec<u8>,
) -> usize {
    let routing_info = RoutingInfo::SenderId { source_sender_id };

    // Build the application header: per-frame metadata entries.
    // This also stamps assigned attempt_ids back into FlowInit frame headers so
    // PTO retransmissions reuse the same attempt_id, and records the sender index
    // on the completion channel so the writer can route FlowInitReset/FlowInitFin
    // through the same socket.
    let total_payload_len = encode_frame_metadata(frames, flow_attempt_id, sender_idx, header_buf);

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
    frames: &mut Queue<Frame>,
    flow_attempt_id: &mut VarInt,
    sender_idx: usize,
    header_buf: &mut Vec<u8>,
) -> usize {
    header_buf.clear();
    let mut total_payload_len = 0usize;

    for frame in frames.iter_mut() {
        stamp_attempt_id(&mut frame.header, flow_attempt_id);
        stamp_init_sender_idx(frame, sender_idx);
        push_frame_metadata(header_buf, &frame.header, frame.payload_len());

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

/// Stamp attempt_id in place for FlowInit frames.
///
/// If the frame's attempt_id is the sentinel `VarInt::MAX`, allocates from the counter
/// and writes it back into the header. On PTO retransmission the header already holds
/// the assigned value, so no new allocation occurs.
fn stamp_attempt_id(header: &mut frame::Header, flow_attempt_id: &mut VarInt) {
    if let frame::Header::FlowInit { attempt_id, .. } = header {
        if *attempt_id == VarInt::MAX {
            *attempt_id = *flow_attempt_id;
            *flow_attempt_id += 1;
        }
    }
}

/// Stamp `init_sender_idx` and `init_attempt_id` on the completion channel of a FlowInit frame.
///
/// Called by the assembler the first time it processes a FlowInit frame so that
/// the writer can later route FlowInitReset/FlowInitFin through the same socket, and
/// include the correct attempt_id in FlowInitReset frames for server-side dedup.
fn stamp_init_sender_idx(frame: &Frame, sender_idx: usize) {
    if let frame::Header::FlowInit { attempt_id, .. } = &frame.header {
        if let Some(completion) = &frame.completion {
            completion.set_init_sender_idx(sender_idx);
            completion.set_init_attempt_id(*attempt_id);
        }
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

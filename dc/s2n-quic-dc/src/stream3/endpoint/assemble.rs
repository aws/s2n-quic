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
    msg::segment,
    packet::{datagram::RoutingInfo, WireVersion},
    socket::pool,
    stream3::{
        endpoint::{inflight, send::Context},
        frame::Frame,
    },
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{buffer, varint::VarInt};
use s2n_quic_platform::features::Gso;

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
    cancelled: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
) -> Option<pool::descriptor::Segments>
where
    Clk: precision::Clock + ?Sized,
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
    let time_sent = now.into();
    let max_segments = gso.max_segments().min(segment::MAX_COUNT);

    let unfilled = pool.alloc()?;

    let mut segment_size: u16 = 0;
    let mut segments_written: u32 = 0;

    let result = unfilled.fill_with(|addr, cmsg, mut payload| {
        addr.set(context.path_secret_entry.data_addr().into());

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
                let remaining_total = segment::MAX_TOTAL as usize - offset.min(segment::MAX_TOTAL as usize);
                if segment_size == 0 {
                    remaining_total.min(mtu as usize)
                } else {
                    remaining_total.min(segment_size as usize)
                }
            };

            if max_segment_len == 0 {
                break;
            }

            // Drain cancelled frames before collecting transmittable ones
            let mut cancelled_queue = Queue::new();
            let mut packet_frames = Queue::new();
            let mut metadata = MetadataEstimate::new(context.flow_attempt_id_counter);

            while let Some(frame) = context.pending.pop_front() {
                if !frame.should_transmit() {
                    cancelled_queue.push_back(frame);
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
                    context.pending.push_front(frame);
                    break;
                }

                metadata = next_metadata;
                packet_frames.push_back(frame);

                if estimated_len == max_segment_len {
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

            debug_assert!(encoded_len <= max_segment_len);

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
    fn with_frame_parts(
        mut self,
        header: &crate::stream3::frame::Header,
        payload_len: usize,
    ) -> Self {
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
        credentials: &crate::credentials::Credentials,
        crypto_tag_len: usize,
    ) -> usize {
        let header_len =
            VarInt::new(self.header_len as u64).expect("header length fits in VarInt");
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
    credentials: &crate::credentials::Credentials,
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
fn frame_metadata_len(header: &crate::stream3::frame::Header, payload_len: usize) -> usize {
    if header.has_payload_length() {
        let payload_len = VarInt::try_from(payload_len as u64).unwrap_or(VarInt::ZERO);
        header.encoding_size() + payload_len.encoding_size()
    } else {
        debug_assert_payload_length_invariant(payload_len);
        header.encoding_size()
    }
}

#[inline]
fn debug_assert_payload_length_invariant(payload_len: usize) {
    debug_assert_eq!(
        payload_len, 0,
        "frames without payload_length must have zero payload"
    );
}

#[inline]
fn push_frame_metadata(
    header_buf: &mut Vec<u8>,
    header: &crate::stream3::frame::Header,
    payload_len: usize,
) {
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

    debug_assert_eq!(enc.len(), entry_size, "frame metadata encoder length mismatch");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        byte_vec::ByteVec,
        clock::testing::Clock,
        counter::Registry,
        packet::datagram::ResetTarget,
        path::secret::map::Entry as PathSecretEntry,
        stream3::frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
    };
    use bolero::check;
    use bytes::Bytes;
    use core::time::Duration;
    use s2n_quic_platform::features::gso::MaxSegments;
    use std::sync::Arc;

    const MAX_TEST_FRAMES: usize = segment::MAX_COUNT * 2;
    const MAX_TEST_PAYLOAD_LEN: usize = 8_500;
    const MIN_TEST_MTU: u16 = 1_200;
    const MAX_TEST_MTU: u16 = 8_950;
    const TEST_CRYPTO_TAG_LEN: usize = 16;

    #[derive(Clone, Debug)]
    struct FrameInput {
        header: Header,
        payload: Vec<u8>,
    }

    impl bolero_generator::TypeGenerator for FrameInput {
        fn generate<D>(driver: &mut D) -> Option<Self>
        where
            D: bolero_generator::Driver,
        {
            use bolero_generator::{TypeGenerator as _, ValueGenerator as _};

            let header = Header::generate(driver)?;
            let payload_len = (0..=MAX_TEST_PAYLOAD_LEN).generate(driver)?;
            let mut payload = Vec::with_capacity(payload_len);
            for _ in 0..payload_len {
                payload.push(<u8 as bolero_generator::TypeGenerator>::generate(driver)?);
            }

            Some(Self { header, payload })
        }
    }

    #[derive(Clone, Debug)]
    struct HarnessInput {
        mtu: u16,
        max_segments: usize,
        source_sender_id: VarInt,
        source_control_port: u16,
        frames: Vec<FrameInput>,
    }

    impl bolero_generator::TypeGenerator for HarnessInput {
        fn generate<D>(driver: &mut D) -> Option<Self>
        where
            D: bolero_generator::Driver,
        {
            use bolero_generator::{TypeGenerator as _, ValueGenerator as _};

            let mtu = (MIN_TEST_MTU..=MAX_TEST_MTU).generate(driver)?;
            let max_segments = (1..=segment::MAX_COUNT).generate(driver)?;
            let source_sender_id = VarInt::generate(driver)?;
            let source_control_port =
                <u16 as bolero_generator::TypeGenerator>::generate(driver)?;

            let mut frames = Vec::with_capacity(MAX_TEST_FRAMES);
            for _ in 0..MAX_TEST_FRAMES {
                frames.push(FrameInput::generate(driver)?);
            }

            Some(Self {
                mtu,
                max_segments,
                source_sender_id,
                source_control_port,
                frames,
            })
        }
    }

    struct CancelledSender(Vec<Queue<Frame>>);

    impl crate::socket::channel::UnboundedSender<Queue<Frame>> for CancelledSender {
        fn send(&mut self, value: Queue<Frame>) -> Result<(), Queue<Frame>> {
            self.0.push(value);
            Ok(())
        }
    }

    fn make_context(mtu: u16) -> (Context, Arc<PathSecretEntry>) {
        let entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);
        entry.update_max_datagram_size(mtu);
        let registry = Registry::new();
        let gauge = registry.register_queue_gauge("test.inflight");
        (Context::new(&entry, gauge), entry)
    }

    fn make_gso(max_segments: usize) -> Gso {
        MaxSegments::try_from(max_segments).unwrap().into()
    }

    fn payload_len(frame: &FrameInput) -> usize {
        if frame.header.has_payload_length() {
            frame.payload.len()
        } else {
            0
        }
    }

    fn is_frame_encodable(
        frame: &FrameInput,
        source_sender_id: VarInt,
        source_control_port: u16,
        credentials: &crate::credentials::Credentials,
        mtu: u16,
    ) -> bool {
        MetadataEstimate::new(VarInt::ZERO)
            .with_frame_parts(&frame.header, payload_len(frame))
            .estimate_packet_len(
                source_sender_id,
                source_control_port,
                VarInt::ZERO,
                credentials,
                TEST_CRYPTO_TAG_LEN,
            )
            <= mtu as usize
    }

    fn to_frame(frame: &FrameInput, entry: &Arc<PathSecretEntry>) -> crate::intrusive_queue::Entry<Frame> {
        let payload = if frame.header.has_payload_length() {
            frame.payload.as_slice()
        } else {
            &[]
        };

        Frame {
            header: frame.header,
            source_sender_id: VarInt::MAX,
            payload: payload_vec(payload),
            path_secret_entry: entry.clone(),
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        }
        .into()
    }

    fn payload_vec(bytes: &[u8]) -> ByteVec {
        let mut payload = ByteVec::new();
        if !bytes.is_empty() {
            payload.push_back(Bytes::copy_from_slice(bytes));
        }
        payload
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Oracle {
        packet_sizes: Vec<u16>,
        remaining_frames: usize,
    }

    fn oracle(
        frames: &[FrameInput],
        source_sender_id: VarInt,
        source_control_port: u16,
        credentials: &crate::credentials::Credentials,
        mtu: u16,
        max_segments: usize,
    ) -> Oracle {
        let mut packet_sizes = Vec::new();
        let mut segment_size = 0u16;
        let mut offset = 0usize;
        let mut packet_number = VarInt::ZERO;
        let mut flow_attempt_id = VarInt::ZERO;
        let mut next_idx = 0usize;

        while next_idx < frames.len() && packet_sizes.len() < max_segments {
            let remaining_total =
                segment::MAX_TOTAL as usize - offset.min(segment::MAX_TOTAL as usize);
            let max_segment_len = if segment_size == 0 {
                remaining_total.min(mtu as usize)
            } else {
                remaining_total.min(segment_size as usize)
            };

            if max_segment_len == 0 {
                break;
            }

            let start_idx = next_idx;
            let mut metadata = MetadataEstimate::new(flow_attempt_id);

            while let Some(frame) = frames.get(next_idx) {
                let next_metadata = metadata.with_frame_parts(&frame.header, payload_len(frame));
                let estimated_len = next_metadata.estimate_packet_len(
                    source_sender_id,
                    source_control_port,
                    packet_number,
                    credentials,
                    TEST_CRYPTO_TAG_LEN,
                );

                if estimated_len > max_segment_len {
                    break;
                }

                metadata = next_metadata;
                next_idx += 1;

                if estimated_len == max_segment_len {
                    break;
                }
            }

            if next_idx == start_idx {
                break;
            }

            let packet_len = metadata.estimate_packet_len(
                source_sender_id,
                source_control_port,
                packet_number,
                credentials,
                TEST_CRYPTO_TAG_LEN,
            ) as u16;
            packet_sizes.push(packet_len);

            if segment_size == 0 {
                segment_size = packet_len;
            }

            offset += segment_size as usize;
            flow_attempt_id = metadata.flow_attempt_id;
            packet_number += 1;

            if packet_len < segment_size {
                break;
            }
        }

        Oracle {
            packet_sizes,
            remaining_frames: frames.len().saturating_sub(next_idx),
        }
    }

    fn assert_gso_invariants(segments: &pool::descriptor::Segments, mtu: u16, max_segments: usize) {
        let sizes = segments.sizes().collect::<Vec<_>>();
        assert!(!sizes.is_empty());
        assert!(sizes.len() <= max_segments);
        assert!(segments.total_payload_len() <= segment::MAX_TOTAL);

        let segment_len = sizes[0];
        assert!(segment_len <= mtu);

        for size in sizes.iter().take(sizes.len().saturating_sub(1)) {
            assert_eq!(*size, segment_len);
        }

        assert!(sizes.last().copied().unwrap() <= segment_len);
        assert_eq!(
            sizes.iter().map(|size| *size as usize).sum::<usize>(),
            segments.total_payload_len() as usize
        );
    }

    #[test]
    fn assemble_accounts_for_header_overhead() {
        let mtu = 256;
        let (mut context, entry) = make_context(mtu);
        let clock = Clock::new(Duration::from_micros(1));
        let gso = make_gso(1);
        let pool = pool::Pool::new(u16::MAX);
        let mut header_buf = Vec::new();
        let mut cancelled = CancelledSender(Vec::new());

        for _ in 0..128 {
            context.push_frame(
                Frame {
                    header: Header::FlowReset {
                        dest_queue_id: VarInt::from_u8(1),
                        stream_id: VarInt::from_u8(1),
                        reset_target: ResetTarget::Both,
                        error_code: VarInt::from_u8(1),
                    },
                    source_sender_id: VarInt::MAX,
                    payload: ByteVec::new(),
                    path_secret_entry: entry.clone(),
                    completion: None,
                    status: TransmissionStatus::default(),
                    ttl: DEFAULT_TTL,
                    transmission_time: None,
                }
                .into(),
            );
        }

        let segments = assemble(
            &mut context,
            &clock,
            VarInt::from_u8(1),
            443,
            &gso,
            &pool,
            &mut header_buf,
            &mut cancelled,
        )
        .expect("frames should assemble");

        assert_gso_invariants(&segments, mtu, gso.max_segments().min(segment::MAX_COUNT));
        assert!(context.has_pending(), "header-heavy frames should spill into another batch");
    }

    #[test]
    fn assemble_fuzz_respects_gso_invariants() {
        check!()
            .with_type::<HarnessInput>()
            .with_test_time(Duration::from_secs(10))
            .for_each(|input| {
                let (mut context, entry) = make_context(input.mtu);
                let frames = input
                    .frames
                    .iter()
                    .filter(|frame| {
                        is_frame_encodable(
                            frame,
                            input.source_sender_id,
                            input.source_control_port,
                            &context.credentials,
                            input.mtu,
                        )
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if frames.is_empty() {
                    return;
                }

                let max_segments = input.max_segments.min(segment::MAX_COUNT);
                let oracle = oracle(
                    &frames,
                    input.source_sender_id,
                    input.source_control_port,
                    &context.credentials,
                    input.mtu,
                    max_segments,
                );
                let clock = Clock::new(Duration::from_micros(1));
                let gso = make_gso(max_segments);
                let pool = pool::Pool::new(u16::MAX);
                let mut header_buf = Vec::new();
                let mut cancelled = CancelledSender(Vec::new());

                for frame in &frames {
                    context.push_frame(to_frame(frame, &entry));
                }

                let segments = assemble(
                    &mut context,
                    &clock,
                    input.source_sender_id,
                    input.source_control_port,
                    &gso,
                    &pool,
                    &mut header_buf,
                    &mut cancelled,
                )
                .expect("assemble should make progress for bounded test inputs");

                assert_gso_invariants(&segments, input.mtu, max_segments);
                assert_eq!(segments.sizes().collect::<Vec<_>>(), oracle.packet_sizes);
                assert_eq!(context.pending.len(), oracle.remaining_frames);
            });
    }

    #[test]
    fn encode_decode_round_trip() {
        use crate::stream3::endpoint::decode;
        use s2n_quic_core::endpoint;

        // Use fake_deterministic so sealer (Client) and opener (Server) share the
        // same underlying secret and therefore the same derived application key.
        let sealer_entry = PathSecretEntry::fake_deterministic(
            "127.0.0.1:8080".parse().unwrap(),
            endpoint::Type::Client,
        );
        let opener_entry = PathSecretEntry::fake_deterministic(
            "127.0.0.1:8080".parse().unwrap(),
            endpoint::Type::Server,
        );
        sealer_entry.update_max_datagram_size(1500);

        let registry = Registry::new();
        let gauge = registry.register_queue_gauge("test.inflight");
        let mut context = Context::new(&sealer_entry, gauge);

        let key_id = context.credentials.key_id;
        let opener = opener_entry.secret().application_opener(key_id);

        let input_frames = vec![
            FrameInput {
                header: Header::FlowData {
                    queue_pair: crate::packet::datagram::QueuePair {
                        source_queue_id: VarInt::from_u8(1),
                        dest_queue_id: VarInt::from_u8(2),
                    },
                    stream_id: VarInt::from_u8(42),
                    offset: VarInt::ZERO,
                    is_fin: false,
                },
                payload: b"hello world".to_vec(),
            },
            FrameInput {
                header: Header::FlowReset {
                    dest_queue_id: VarInt::from_u8(3),
                    stream_id: VarInt::from_u8(10),
                    reset_target: ResetTarget::Both,
                    error_code: VarInt::from_u8(1),
                },
                payload: vec![],
            },
            FrameInput {
                header: Header::FlowData {
                    queue_pair: crate::packet::datagram::QueuePair {
                        source_queue_id: VarInt::from_u8(4),
                        dest_queue_id: VarInt::from_u8(5),
                    },
                    stream_id: VarInt::from_u8(20),
                    offset: VarInt::from_u8(11),
                    is_fin: true,
                },
                payload: b"fin frame".to_vec(),
            },
        ];

        for frame in &input_frames {
            context.push_frame(to_frame(frame, &sealer_entry));
        }

        let mut buf = vec![0u8; 65536];
        let mut header_buf = Vec::new();
        let tag_len = crate::crypto::seal::Application::tag_len(&context.sealer);
        let encoded_len = encode_segment(
            &mut buf,
            443, // source_control_port
            VarInt::from_u8(7), // source_sender_id
            context.next_packet_number,
            &context.sealer,
            &context.credentials,
            &mut context.flow_attempt_id_counter,
            &context.pending,
            &mut header_buf,
        );
        assert!(encoded_len > 0, "must encode at least something");

        // Decode the packet
        let decode_buf = s2n_codec::DecoderBufferMut::new(&mut buf[..encoded_len]);
        let (mut packet, _) =
            crate::packet::datagram::decoder::Packet::decode(decode_buf, (), tag_len)
                .expect("packet must decode cleanly");

        assert!(
            matches!(
                packet.routing_info(),
                crate::packet::datagram::RoutingInfo::SenderId { .. }
            ),
            "multi-frame packets use SenderId routing"
        );

        // Decrypt in place
        packet
            .decrypt_in_place(&opener)
            .expect("decryption must succeed with matching key pair");

        // Decode the frame metadata; pair each (header, payload_len) with the
        // corresponding payload bytes from the decrypted payload region.
        let app_header = packet.application_header();
        let payload_bytes = packet.payload();
        let decoded_frames: Vec<_> = {
            let mut offset = 0usize;
            let mut result = Vec::new();
            for item in decode::decode_frames(app_header) {
                let (header, payload_len) = item.expect("frame metadata must decode");
                result.push((header, &payload_bytes[offset..offset + payload_len]));
                offset += payload_len;
            }
            assert_eq!(offset, payload_bytes.len(), "all payload bytes must be consumed");
            result
        };

        assert_eq!(
            decoded_frames.len(),
            input_frames.len(),
            "decoded frame count must match"
        );

        for (i, ((header, payload), original)) in
            decoded_frames.iter().zip(input_frames.iter()).enumerate()
        {
            assert_eq!(*header, original.header, "frame[{i}] header mismatch");
            let expected_payload = if original.header.has_payload_length() {
                &original.payload[..]
            } else {
                &[][..]
            };
            assert_eq!(*payload, expected_payload, "frame[{i}] payload mismatch");
        }
    }

    #[test]
    fn encode_decode_fuzz_round_trip() {
        use crate::stream3::endpoint::decode;
        use s2n_quic_core::endpoint;

        check!()
            .with_type::<HarnessInput>()
            .with_test_time(Duration::from_secs(10))
            .for_each(|input| {
                let sealer_entry = PathSecretEntry::fake_deterministic(
                    "127.0.0.1:8080".parse().unwrap(),
                    endpoint::Type::Client,
                );
                let opener_entry = PathSecretEntry::fake_deterministic(
                    "127.0.0.1:8080".parse().unwrap(),
                    endpoint::Type::Server,
                );
                sealer_entry.update_max_datagram_size(input.mtu);

                let registry = Registry::new();
                let gauge = registry.register_queue_gauge("test.inflight");
                let context = Context::new(&sealer_entry, gauge);

                let key_id = context.credentials.key_id;
                let opener = opener_entry.secret().application_opener(key_id);
                let tag_len = crate::crypto::seal::Application::tag_len(&context.sealer);

                // Simulate the assembler: pick frames that fit together in one packet.
                let mut packet_inputs: Vec<&FrameInput> = Vec::new();
                let mut meta = MetadataEstimate::new(VarInt::ZERO);
                for frame in &input.frames {
                    if !is_frame_encodable(
                        frame,
                        input.source_sender_id,
                        input.source_control_port,
                        &context.credentials,
                        input.mtu,
                    ) {
                        continue;
                    }
                    let next_meta = meta.with_frame_parts(&frame.header, payload_len(frame));
                    let est = next_meta.estimate_packet_len(
                        input.source_sender_id,
                        input.source_control_port,
                        context.next_packet_number,
                        &context.credentials,
                        tag_len,
                    );
                    if est > input.mtu as usize {
                        break;
                    }
                    meta = next_meta;
                    packet_inputs.push(frame);
                    if est == input.mtu as usize {
                        break;
                    }
                }

                if packet_inputs.is_empty() {
                    return;
                }

                // Build a queue of exactly the frames that fit in one packet.
                let mut packet_frames = Queue::new();
                for f in &packet_inputs {
                    packet_frames.push_back(to_frame(f, &sealer_entry));
                }

                let mut buf = vec![0u8; 65536];
                let mut header_buf = Vec::new();
                let mut flow_attempt_id = VarInt::ZERO;

                let encoded_len = encode_segment(
                    &mut buf,
                    input.source_control_port,
                    input.source_sender_id,
                    context.next_packet_number,
                    &context.sealer,
                    &context.credentials,
                    &mut flow_attempt_id,
                    &packet_frames,
                    &mut header_buf,
                );

                if encoded_len == 0 {
                    return;
                }

                // Decode the outer datagram packet
                let decode_buf = s2n_codec::DecoderBufferMut::new(&mut buf[..encoded_len]);
                let (mut packet, _) =
                    crate::packet::datagram::decoder::Packet::decode(decode_buf, (), tag_len)
                        .expect("encoded packet must decode");

                // Decrypt in place
                packet
                    .decrypt_in_place(&opener)
                    .expect("decrypt must succeed with matched key pair");

                // Decode the frame metadata region
                let app_header = packet.application_header();
                let payload_bytes = packet.payload();
                let decoded: Vec<_> = {
                    let mut offset = 0usize;
                    let mut result = Vec::new();
                    for item in decode::decode_frames(app_header) {
                        let (header, payload_len) = item.expect("frame metadata must decode after decryption");
                        result.push((header, &payload_bytes[offset..offset + payload_len]));
                        offset += payload_len;
                    }
                    assert_eq!(offset, payload_bytes.len(), "all payload bytes must be consumed");
                    result
                };

                // Verify frames match
                assert_eq!(
                    decoded.len(),
                    packet_inputs.len(),
                    "decoded frame count must match"
                );

                for (i, ((header, payload), orig)) in decoded.iter().zip(packet_inputs.iter()).enumerate() {
                    assert_eq!(*header, orig.header, "frame[{i}] header mismatch");
                    let exp_payload = if orig.header.has_payload_length() {
                        &orig.payload[..]
                    } else {
                        &[][..]
                    };
                    assert_eq!(*payload, exp_payload, "frame[{i}] payload mismatch");
                }
            });
    }
}

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
        use bolero_generator::ValueGenerator as _;

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
        use bolero_generator::ValueGenerator as _;

        let mtu = (MIN_TEST_MTU..=MAX_TEST_MTU).generate(driver)?;
        let max_segments = (1..=segment::MAX_COUNT).generate(driver)?;
        let source_sender_id = VarInt::generate(driver)?;
        let source_control_port = <u16 as bolero_generator::TypeGenerator>::generate(driver)?;

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

// Queue<T> implements UnboundedSender<Entry<T>> via push_back, so we use it directly.

fn make_context(mtu: u16, registry: &Registry) -> (Context, Arc<PathSecretEntry>) {
    let peer: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let entry = PathSecretEntry::fake(peer, None);
    entry.update_max_datagram_size(mtu);
    entry.set_peer_data_addrs(&[peer]);
    let inflight_gauge = registry.register_queue_gauge("test.inflight");
    let ack_gauge = registry.register_queue_gauge("test.ack");
    let pending_gauge = registry.register_queue_gauge("test.pending");
    (
        Context::new(&entry, inflight_gauge, ack_gauge, pending_gauge, 0).unwrap(),
        entry,
    )
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
    MetadataEstimate::new()
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

fn to_frame(
    frame: &FrameInput,
    entry: &Arc<PathSecretEntry>,
) -> crate::intrusive_queue::Entry<Frame> {
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
    let mut next_idx = 0usize;

    while next_idx < frames.len() && packet_sizes.len() < max_segments {
        let remaining_total = segment::MAX_TOTAL as usize - offset.min(segment::MAX_TOTAL as usize);
        let max_segment_len = if segment_size == 0 {
            remaining_total.min(mtu as usize)
        } else {
            remaining_total.min(segment_size as usize)
        };

        if max_segment_len == 0 {
            break;
        }

        let start_idx = next_idx;
        let mut metadata = MetadataEstimate::new();

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
    let registry = Registry::new();
    let (mut context, entry) = make_context(mtu, &registry);
    let clock = Clock::new(Duration::from_micros(1));
    let gso = make_gso(1);
    let pool = pool::Pool::new(u16::MAX);
    let mut header_buf = Vec::new();
    let mut cancelled = Queue::new();
    let mut ack_completions = Queue::new();

    for _ in 0..128 {
        context.push_back_frame(
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

    let registry = Registry::new();
    let counters = AssemblerCounters::new(&registry);
    let segments = assemble(
        &mut context,
        &clock,
        VarInt::from_u8(1),
        443,
        &gso,
        &pool,
        &mut header_buf,
        &mut cancelled,
        &mut ack_completions,
        &counters,
    )
    .expect("frames should assemble");

    assert_gso_invariants(&segments, mtu, gso.max_segments().min(segment::MAX_COUNT));
    assert!(
        context.has_pending(),
        "header-heavy frames should spill into another batch"
    );
}

#[test]
fn assemble_fuzz_respects_gso_invariants() {
    check!()
        .with_type::<HarnessInput>()
        .with_test_time(Duration::from_secs(10))
        .for_each(|input| {
            let registry = Registry::new();
            let (mut context, entry) = make_context(input.mtu, &registry);
            let mut frames = input
                .frames
                .iter()
                .filter(|frame| {
                    !matches!(frame.header, Header::Ack { .. })
                        && is_frame_encodable(
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

            // Sort by priority to match the assembler's pending-queue drain order.
            frames.sort_by_key(|f| f.header.priority().as_index());

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
            let mut cancelled = Queue::new();
            let mut ack_completions = Queue::new();

            for frame in &frames {
                context.push_back_frame(to_frame(frame, &entry));
            }

            let registry = Registry::new();
            let counters = AssemblerCounters::new(&registry);
            let segments = assemble(
                &mut context,
                &clock,
                input.source_sender_id,
                input.source_control_port,
                &gso,
                &pool,
                &mut header_buf,
                &mut cancelled,
                &mut ack_completions,
                &counters,
            )
            .expect("assemble should make progress for bounded test inputs");

            assert_gso_invariants(&segments, input.mtu, max_segments);
            assert_eq!(segments.sizes().collect::<Vec<_>>(), oracle.packet_sizes);
            assert_eq!(context.pending_count(), oracle.remaining_frames);
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
    let peer: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
    sealer_entry.set_peer_data_addrs(&[peer]);

    let registry = Registry::new();
    let inflight_gauge = registry.register_queue_gauge("test.inflight");
    let ack_gauge = registry.register_queue_gauge("test.ack");
    let pending_gauge = registry.register_queue_gauge("test.pending");
    let context = Context::new(&sealer_entry, inflight_gauge, ack_gauge, pending_gauge, 0).unwrap();

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

    let mut packet_frames = Queue::new();
    for frame in &input_frames {
        packet_frames.push_back(to_frame(frame, &sealer_entry));
    }

    let mut buf = vec![0u8; 65536];
    let mut header_buf = Vec::new();
    let mut flow_attempt_id = VarInt::ZERO;
    let tag_len = crate::crypto::seal::Application::tag_len(&context.sealer);
    let encoded_len = encode_segment(
        &mut buf,
        443,                // source_control_port
        VarInt::from_u8(7), // source_sender_id
        context.next_packet_number,
        &context.sealer,
        &context.credentials,
        &mut flow_attempt_id,
        &mut packet_frames,
        &mut header_buf,
    );
    assert!(encoded_len > 0, "must encode at least something");

    // Decode the packet
    let decode_buf = s2n_codec::DecoderBufferMut::new(&mut buf[..encoded_len]);
    let (mut packet, _) = crate::packet::datagram::decoder::Packet::decode(decode_buf, (), tag_len)
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
        assert_eq!(
            offset,
            payload_bytes.len(),
            "all payload bytes must be consumed"
        );
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
            let peer: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
            sealer_entry.set_peer_data_addrs(&[peer]);

            let registry = Registry::new();
            let inflight_gauge = registry.register_queue_gauge("test.inflight");
            let ack_gauge = registry.register_queue_gauge("test.ack");
            let pending_gauge = registry.register_queue_gauge("test.pending");
            let context =
                Context::new(&sealer_entry, inflight_gauge, ack_gauge, pending_gauge, 0).unwrap();

            let key_id = context.credentials.key_id;
            let opener = opener_entry.secret().application_opener(key_id);
            let tag_len = crate::crypto::seal::Application::tag_len(&context.sealer);

            // Simulate the assembler: pick frames that fit together in one packet.
            let mut packet_inputs: Vec<&FrameInput> = Vec::new();
            let mut meta = MetadataEstimate::new();
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
                &mut packet_frames,
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
                    let (header, payload_len) =
                        item.expect("frame metadata must decode after decryption");
                    result.push((header, &payload_bytes[offset..offset + payload_len]));
                    offset += payload_len;
                }
                assert_eq!(
                    offset,
                    payload_bytes.len(),
                    "all payload bytes must be consumed"
                );
                result
            };

            // Verify frames match
            assert_eq!(
                decoded.len(),
                packet_inputs.len(),
                "decoded frame count must match"
            );

            for (i, ((header, payload), orig)) in
                decoded.iter().zip(packet_inputs.iter()).enumerate()
            {
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

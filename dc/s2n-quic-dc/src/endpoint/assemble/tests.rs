// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    byte_vec::ByteVec,
    counter::Registry,
    endpoint::{
        frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
        id::Id,
    },
    packet::datagram::ResetTarget,
    path::secret::map::Entry as PathSecretEntry,
    socket::{
        channel::ImmediateQueueStatus,
        pool::{self, descriptor::SyncRecycler},
    },
    time::testing::Clock,
};
use bolero::check;
use bytes::Bytes;
use core::time::Duration;
use s2n_quic_platform::features::gso::MaxSegments;
use std::sync::Arc;

/// Build an unused credit pool for assemble tests that don't exercise credit accounting.
/// The pool's release() is a no-op when nothing is acquired against it; tests that exercise
/// the admit-path release just observe `release_calls == 0` here, which is also no-op.
fn unused_credit_pool() -> crate::credit::Pool {
    crate::credit::Pool::new(crate::credit::Config::default())
}

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
    let entry = PathSecretEntry::builder(peer).build();
    entry.update_max_datagram_size(mtu);
    entry.set_peer_data_addrs(&[peer]);
    let inflight_gauge = registry.register_queue_gauge("test.inflight");
    let ack_gauge = registry.register_queue_gauge("test.ack");
    let pending_gauge = registry.register_queue_gauge("test.pending");
    (
        Context::new(
            &entry,
            inflight_gauge,
            ack_gauge,
            pending_gauge,
            crate::endpoint::id::LocalSenderId::from_index(0),
            &crate::time::bach::Clock::default(),
        )
        .unwrap(),
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
            crate::endpoint::id::LocalSenderId::new(source_sender_id),
            source_control_port,
            credentials,
            TEST_CRYPTO_TAG_LEN,
        )
        <= mtu as usize
}

fn to_frame(frame: &FrameInput, entry: &Arc<PathSecretEntry>) -> crate::intrusive::Entry<Frame> {
    let payload = if frame.header.has_payload_length() {
        frame.payload.as_slice()
    } else {
        &[]
    };

    Frame {
        header: frame.header,
        payload: payload_vec(payload),
        path_secret_entry: entry.clone(),
        completion: None,
        status: TransmissionStatus::default(),
        ttl: DEFAULT_TTL,
        enqueued_at: None,
        flow_credits: 0,
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
                crate::endpoint::id::LocalSenderId::new(source_sender_id),
                source_control_port,
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

        // The estimate uses VarInt::MAX for PN overhead, but the oracle needs to
        // predict the actual encoded size which uses the real (smaller) PN.
        // Since the assembler starts at PN 0 and increments by 1 per segment,
        // the real PN for this packet is packet_sizes.len().
        let real_pn = VarInt::new(packet_sizes.len() as u64).unwrap();
        let pn_saving = VarInt::MAX.encoding_size() - real_pn.encoding_size();
        let estimated = metadata.estimate_packet_len(
            crate::endpoint::id::LocalSenderId::new(source_sender_id),
            source_control_port,
            credentials,
            TEST_CRYPTO_TAG_LEN,
        );
        let packet_len = (estimated - pn_saving) as u16;
        packet_sizes.push(packet_len);

        if segment_size == 0 {
            segment_size = packet_len;
        }

        offset += segment_size as usize;

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
    let (mut freed_batch_tx, _freed_batch_rx) = crate::queue::freed_batch_channel();

    for _ in 0..128 {
        context.push_back_frame(
            Frame {
                header: Header::QueueReset {
                    dest_queue_id: VarInt::from_u8(1),
                    binding_id: VarInt::from_u8(1),
                    reset_target: ResetTarget::Both,
                    error_code: VarInt::from_u8(1),
                    dest_acceptor_id: None,
                },
                payload: ByteVec::new(),
                path_secret_entry: entry.clone(),
                completion: None,
                status: TransmissionStatus::default(),
                ttl: DEFAULT_TTL,
                enqueued_at: None,
                flow_credits: 0,
            }
            .into(),
        );
    }

    let registry = Registry::new();
    let counters = AssemblerCounters::new(&registry);
    let segments = assemble(
        &mut context,
        ImmediateQueueStatus::Empty, // no more immediate items
        &clock,
        crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(1)),
        443,
        &gso,
        pool.alloc::<SyncRecycler>().expect("pool alloc failed"),
        &mut header_buf,
        &mut cancelled,
        &mut ack_completions,
        &mut freed_batch_tx,
        &counters,
        &crate::endpoint::counters::Send::new(
            &crate::counter::Registry::default(),
            crate::endpoint::id::LocalSenderId::from_index(0),
        ),
        &unused_credit_pool(),
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
            let (mut freed_batch_tx, _freed_batch_rx) = crate::queue::freed_batch_channel();

            for frame in &frames {
                context.push_back_frame(to_frame(frame, &entry));
            }

            let registry = Registry::new();
            let counters = AssemblerCounters::new(&registry);
            let segments = assemble(
                &mut context,
                ImmediateQueueStatus::Empty, // no more immediate items
                &clock,
                crate::endpoint::id::LocalSenderId::new(input.source_sender_id),
                input.source_control_port,
                &gso,
                pool.alloc::<SyncRecycler>().expect("pool alloc failed"),
                &mut header_buf,
                &mut cancelled,
                &mut ack_completions,
                &mut freed_batch_tx,
                &counters,
                &crate::endpoint::counters::Send::new(
                    &crate::counter::Registry::default(),
                    crate::endpoint::id::LocalSenderId::from_index(0),
                ),
                &unused_credit_pool(),
            )
            .expect("assemble should make progress for bounded test inputs");

            assert_gso_invariants(&segments, input.mtu, max_segments);
            assert_eq!(segments.sizes().collect::<Vec<_>>(), oracle.packet_sizes);
            assert_eq!(context.pending_count(), oracle.remaining_frames);
        });
}

#[test]
fn encode_decode_round_trip() {
    use crate::stream::endpoint::decode;
    use s2n_quic_core::endpoint;

    // Use fake_deterministic so sealer (Client) and opener (Server) share the
    // same underlying secret and therefore the same derived application key.
    let sealer_entry = PathSecretEntry::builder("127.0.0.1:8080".parse().unwrap())
        .endpoint_type(endpoint::Type::Client)
        .build();
    let opener_entry = PathSecretEntry::builder("127.0.0.1:8080".parse().unwrap())
        .endpoint_type(endpoint::Type::Server)
        .build();
    sealer_entry.update_max_datagram_size(1500);
    let peer: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
    sealer_entry.set_peer_data_addrs(&[peer]);

    let registry = Registry::new();
    let inflight_gauge = registry.register_queue_gauge("test.inflight");
    let ack_gauge = registry.register_queue_gauge("test.ack");
    let pending_gauge = registry.register_queue_gauge("test.pending");
    let context = Context::new(
        &sealer_entry,
        inflight_gauge,
        ack_gauge,
        pending_gauge,
        crate::endpoint::id::LocalSenderId::from_index(0),
        &crate::time::bach::Clock::default(),
    )
    .unwrap();

    let key_id = context.credentials.key_id;
    let opener = opener_entry.secret().application_opener(key_id);

    let input_frames = vec![
        FrameInput {
            header: Header::QueueData {
                queue_pair: crate::packet::datagram::QueuePair {
                    source_queue_id: VarInt::from_u8(1),
                    dest_queue_id: VarInt::from_u8(2),
                },
                binding_id: VarInt::from_u8(42),
                offset: VarInt::ZERO,
                is_fin: false,
                dest_acceptor_id: None,
                priority: crate::credit::Priority::default(),
            },
            payload: b"hello world".to_vec(),
        },
        FrameInput {
            header: Header::QueueReset {
                dest_queue_id: VarInt::from_u8(3),
                binding_id: VarInt::from_u8(10),
                reset_target: ResetTarget::Both,
                error_code: VarInt::from_u8(1),
                dest_acceptor_id: None,
            },
            payload: vec![],
        },
        FrameInput {
            header: Header::QueueData {
                queue_pair: crate::packet::datagram::QueuePair {
                    source_queue_id: VarInt::from_u8(4),
                    dest_queue_id: VarInt::from_u8(5),
                },
                binding_id: VarInt::from_u8(20),
                offset: VarInt::from_u8(11),
                is_fin: true,
                dest_acceptor_id: None,
                priority: crate::credit::Priority::default(),
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
    let tag_len = crate::crypto::seal::Application::tag_len(&context.sealer);
    let encoded_len = encode_segment(
        &mut buf,
        443,                                                         // source_control_port
        crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(7)), // source_sender_id
        context.next_packet_number,
        &context.sealer,
        &context.credentials,
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
fn assemble_probe_fuzz() {
    use crate::stream::endpoint::send::ProbeState;

    // VarInt encoding boundaries where adding 4 to a value near the boundary causes
    // the encoding to grow: 1→2 bytes at 64, 2→4 bytes at 16384, 4→8 bytes at 1073741824.
    const VARINT_BOUNDARIES: [u64; 3] = [64, 16_384, 1_073_741_824];

    #[derive(Clone, Debug)]
    struct ProbeInput {
        mtu: u16,
        max_segments: usize,
        source_sender_id: VarInt,
        source_control_port: u16,
        initial_pn: VarInt,
        frames: Vec<FrameInput>,
    }

    impl bolero_generator::TypeGenerator for ProbeInput {
        fn generate<D>(driver: &mut D) -> Option<Self>
        where
            D: bolero_generator::Driver,
        {
            use bolero_generator::ValueGenerator as _;

            let mtu = (MIN_TEST_MTU..=MAX_TEST_MTU).generate(driver)?;
            let max_segments = (1..=segment::MAX_COUNT).generate(driver)?;
            let source_sender_id = VarInt::generate(driver)?;
            let source_control_port = <u16 as bolero_generator::TypeGenerator>::generate(driver)?;

            // Bias the starting PN toward VarInt boundaries where the bug manifests.
            let boundary_idx = (0..VARINT_BOUNDARIES.len()).generate(driver)?;
            let boundary = VARINT_BOUNDARIES[boundary_idx];
            let offset = (0u64..=8).generate(driver)?;
            let initial_pn = VarInt::new(boundary.saturating_sub(offset)).unwrap_or(VarInt::ZERO);

            let frame_count = (1usize..=8).generate(driver)?;
            let mut frames = Vec::with_capacity(frame_count);
            for _ in 0..frame_count {
                frames.push(FrameInput::generate(driver)?);
            }

            Some(Self {
                mtu,
                max_segments,
                source_sender_id,
                source_control_port,
                initial_pn,
                frames,
            })
        }
    }

    check!()
        .with_type::<ProbeInput>()
        .with_test_time(Duration::from_secs(10))
        .for_each(|input| {
            let registry = Registry::new();
            let (mut context, entry) = make_context(input.mtu, &registry);
            context.next_packet_number = input.initial_pn;

            let frames: Vec<_> = input
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
                .collect();
            if frames.is_empty() {
                return;
            }

            let clock = Clock::new(Duration::from_micros(1));
            let gso = make_gso(input.max_segments);
            let pool = pool::Pool::new(u16::MAX);
            let mut header_buf = Vec::new();
            let mut cancelled = Queue::new();
            let mut ack_completions = Queue::new();
            let (mut freed_batch_tx, _freed_batch_rx) = crate::queue::freed_batch_channel();

            for frame in &frames {
                context.push_back_frame(to_frame(frame, &entry));
            }

            // Phase 1: normal assembly — puts frames into inflight.
            let registry2 = Registry::new();
            let counters = AssemblerCounters::new(&registry2);
            let _segments = assemble::<SyncRecycler, _>(
                &mut context,
                ImmediateQueueStatus::Empty, // no more immediate items
                &clock,
                crate::endpoint::id::LocalSenderId::new(input.source_sender_id),
                input.source_control_port,
                &gso,
                pool.alloc::<SyncRecycler>().expect("pool alloc failed"),
                &mut header_buf,
                &mut cancelled,
                &mut ack_completions,
                &mut freed_batch_tx,
                &counters,
                &crate::endpoint::counters::Send::new(
                    &crate::counter::Registry::default(),
                    crate::endpoint::id::LocalSenderId::from_index(0),
                ),
                &unused_credit_pool(),
            );

            if !context.inflight.has_inflight() {
                return;
            }

            // Phase 2: request a probe and reassemble — exercises the probe path.
            context.pto.probe_state = ProbeState::ProbeTwice;

            let result = assemble(
                &mut context,
                ImmediateQueueStatus::Empty, // no more immediate items
                &clock,
                crate::endpoint::id::LocalSenderId::new(input.source_sender_id),
                input.source_control_port,
                &gso,
                pool.alloc::<SyncRecycler>().expect("pool alloc failed"),
                &mut header_buf,
                &mut cancelled,
                &mut ack_completions,
                &mut freed_batch_tx,
                &counters,
                &crate::endpoint::counters::Send::new(
                    &crate::counter::Registry::default(),
                    crate::endpoint::id::LocalSenderId::from_index(0),
                ),
                &unused_credit_pool(),
            );

            // If a probe was assembled, verify GSO invariants.
            if let Some(segments) = result {
                assert_gso_invariants(
                    &segments,
                    input.mtu,
                    input.max_segments.min(segment::MAX_COUNT),
                );
            }
        });
}

#[test]
fn encode_decode_fuzz_round_trip() {
    use crate::stream::endpoint::decode;
    use s2n_quic_core::endpoint;

    check!()
        .with_type::<HarnessInput>()
        .with_test_time(Duration::from_secs(10))
        .for_each(|input| {
            let sealer_entry = PathSecretEntry::builder("127.0.0.1:8080".parse().unwrap())
                .endpoint_type(endpoint::Type::Client)
                .build();
            let opener_entry = PathSecretEntry::builder("127.0.0.1:8080".parse().unwrap())
                .endpoint_type(endpoint::Type::Server)
                .build();
            sealer_entry.update_max_datagram_size(input.mtu);
            let peer: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
            sealer_entry.set_peer_data_addrs(&[peer]);

            let registry = Registry::new();
            let inflight_gauge = registry.register_queue_gauge("test.inflight");
            let ack_gauge = registry.register_queue_gauge("test.ack");
            let pending_gauge = registry.register_queue_gauge("test.pending");
            let context = Context::new(
                &sealer_entry,
                inflight_gauge,
                ack_gauge,
                pending_gauge,
                crate::endpoint::id::LocalSenderId::from_index(0),
                &crate::time::bach::Clock::default(),
            )
            .unwrap();

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
                    crate::endpoint::id::LocalSenderId::new(input.source_sender_id),
                    input.source_control_port,
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

            let encoded_len = encode_segment(
                &mut buf,
                input.source_control_port,
                crate::endpoint::id::LocalSenderId::new(input.source_sender_id),
                context.next_packet_number,
                &context.sealer,
                &context.credentials,
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
                // priority is encoded only on init frames; on non-init frames the
                // decoder reconstructs the default value, so canonicalize before
                // comparing.
                assert_eq!(
                    *header,
                    orig.header.canonicalize_for_wire(),
                    "frame[{i}] header mismatch"
                );
                let exp_payload = if orig.header.has_payload_length() {
                    &orig.payload[..]
                } else {
                    &[][..]
                };
                assert_eq!(*payload, exp_payload, "frame[{i}] payload mismatch");
            }
        });
}

/// Builds a single ack-eliciting QueueData frame carrying `completion`, so the test
/// can later cancel it (drop/cancel the receiver) and observe `should_transmit()`
/// flip to false. Mirrors the frame shape produced by the writer for stream data.
fn make_data_frame(
    entry: &Arc<PathSecretEntry>,
    completion: crate::endpoint::frame::CompletionSender,
) -> crate::intrusive::Entry<Frame> {
    use crate::packet::datagram::QueuePair;

    let mut payload = ByteVec::new();
    payload.push_back(Bytes::from_static(b"hello"));
    Frame {
        header: Header::QueueData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            is_fin: false,
            dest_acceptor_id: None,
            priority: crate::credit::Priority::default(),
        },
        payload,
        path_secret_entry: entry.clone(),
        completion: Some(completion),
        status: TransmissionStatus::default(),
        ttl: DEFAULT_TTL,
        enqueued_at: None,
        flow_credits: 0,
    }
    .into()
}

/// Reproduces the "orphaned probe shell" defect at the smallest scale that exercises
/// the real assembler: a shell entry whose probe chain tail is removed by the
/// cancelled-frame path in `assemble_probe`, leaving the shell dangling forever.
///
/// Background — the inflight map links PTO probes with *forward-only* pointers:
/// when PN N is retransmitted as a probe at PN X, N becomes a "shell"
/// (`probed_to = Some(X)`, frames emptied, bytes zeroed) and X holds the live frames.
/// Reaping a shell requires either ACK processing following the chain forward, or
/// loss detection sweeping the contiguous lost prefix — both run only from
/// `process_ack`, i.e. only when an ACK arrives.
///
/// The escape: if, on a later PTO, the probe tail's frames have all been cancelled
/// (the writer dropped, so `should_transmit()` is false), `assemble_probe` removes
/// the tail directly via `inflight.remove(old_pn)` (assemble.rs, the `!has_frame`
/// branch) — in the *send* path, with no ACK and no loss detection. The predecessor
/// shell still points at the now-removed tail. Nothing ever follows or sweeps it, so
/// it lingers: `has_inflight()` stays true while `bytes_in_flight == 0`.
///
/// This is the deterministic core of the production "routing asymmetry" warning,
/// which fires from the idle-wheel handler when a context is idle
/// (`bytes_in_flight == 0`) yet `has_inflight()` is still true.
///
/// The test asserts the CORRECT behavior: once the probe tail is removed because
/// its frames were cancelled, no inflight state should remain (the context must be
/// able to drain). On the current code it FAILS, demonstrating the bug — the
/// predecessor shell is left dangling. The fix (drop predecessor shells when the
/// cancelled-tail path removes their tail) makes it pass.
#[test]
fn orphaned_shell_survives_cancelled_probe_tail() {
    use crate::stream::endpoint::send::ProbeState;

    let mtu = 1500;
    let registry = Registry::new();
    let (mut context, entry) = make_context(mtu, &registry);

    let clock = Clock::new(Duration::from_micros(1));
    let gso = make_gso(1);
    let pool = pool::Pool::new(u16::MAX);
    let mut header_buf = Vec::new();
    let mut cancelled = Queue::new();
    let mut ack_completions = Queue::new();
    let (mut freed_batch_tx, _freed_batch_rx) = crate::queue::freed_batch_channel();

    let send_counters = crate::endpoint::counters::Send::new(
        &crate::counter::Registry::default(),
        crate::endpoint::id::LocalSenderId::from_index(0),
    );
    let source_sender_id = crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(1));
    let source_control_port = 443;

    let credit_pool = unused_credit_pool();
    // Helper to run one assembly round with the shared arguments.
    macro_rules! run_assemble {
        () => {
            assemble::<SyncRecycler, _>(
                &mut context,
                ImmediateQueueStatus::Empty,
                &clock,
                source_sender_id,
                source_control_port,
                &gso,
                pool.alloc::<SyncRecycler>().expect("pool alloc failed"),
                &mut header_buf,
                &mut cancelled,
                &mut ack_completions,
                &mut freed_batch_tx,
                &AssemblerCounters::new(&registry),
                &send_counters,
                &credit_pool,
            )
        };
    }

    // ── Round 1: send the data frame at PN 0 (held alive by `completion_rx`). ──
    let completion_rx = crate::endpoint::frame::completion_channel();
    context.push_back_frame(make_data_frame(&entry, completion_rx.sender()));
    let _ = run_assemble!().expect("initial data should assemble");
    assert!(context.inflight.has_inflight(), "PN 0 is in flight");
    let range = context.inflight.get_range();
    let shell_pn = range.start();
    assert_eq!(
        range.start(),
        range.end(),
        "exactly one inflight entry (PN 0)"
    );
    assert!(
        context.cca.bytes_in_flight() > 0,
        "live packet contributes bytes_in_flight"
    );

    // ── Round 2: PTO probe — retransmits PN 0's frame at PN 1. ──
    // PN 0 becomes a shell (probed_to = Some(PN 1)); PN 1 holds the live frame.
    context.pto.probe_state = ProbeState::ProbeTwice;
    let _ = run_assemble!();
    assert!(
        context.inflight.get_range().end().as_u64() > shell_pn.as_u64(),
        "probe created a higher PN tail (PN 0 -> PN 1 chain)"
    );
    // Only the live tail's bytes count; the shell's bytes were released.
    let tail_bytes = context.cca.bytes_in_flight();
    assert!(tail_bytes > 0, "probe tail contributes bytes_in_flight");
    assert_eq!(
        context.inflight.sum_sent_bytes(),
        tail_bytes,
        "inflight byte accounting matches CCA (shell zeroed, tail counted)"
    );

    // ── Cancel the writer: the in-flight frame must no longer be transmitted. ──
    completion_rx.cancel();

    // ── Round 3: PTO probe again. `assemble_probe` pulls the tail's frame, finds it
    // cancelled, and removes the tail directly via the `!has_frame` branch — WITHOUT
    // running ACK or loss detection. The predecessor shell at PN 0 is left dangling.
    context.pto.probe_state = ProbeState::ProbeTwice;
    let _ = run_assemble!();

    // ── Correct behavior: removing the cancelled probe tail must leave no dangling
    // shell. The context has nothing left to send and must be able to drain.
    //
    // On the current (buggy) code these assertions FAIL: the predecessor shell at
    // PN 0 still points at the removed tail (PN 1), so `has_inflight()` stays true
    // with zero bytes — the exact production "routing asymmetry" signature
    // (has_inflight() true, bytes_in_flight == 0, a lone shell with no frames to
    // probe, so the PTO can never make progress and the context never drains).
    //
    // The fix: when the cancelled-tail path in `assemble_probe` removes a tail,
    // it must also drop any predecessor shells whose `probed_to` pointed at it.
    assert!(
        !context.inflight.has_inflight(),
        "orphaned shell at PN {}: tail was removed as cancelled but the shell still \
         keeps has_inflight() true with bytes_in_flight={} (sum_sent_bytes={}); the \
         context can never drain",
        shell_pn.as_u64(),
        context.cca.bytes_in_flight(),
        context.inflight.sum_sent_bytes(),
    );
    assert_eq!(
        context.cca.bytes_in_flight(),
        0,
        "no bytes should remain in flight once the only frame was cancelled"
    );
}

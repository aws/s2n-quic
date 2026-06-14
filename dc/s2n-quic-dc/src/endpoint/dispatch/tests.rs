// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    packet::datagram::{self, QueuePair},
    path::secret::map::Entry as PathSecretEntry,
    socket::pool::{self, descriptor::SyncRecycler},
};
use s2n_codec::{DecoderBufferMut, Encoder as _};
use s2n_quic_core::endpoint;
use std::net::SocketAddr;

const TAG_LEN: usize = 16;

/// A single non-init QueueMsg frame routed at `(dest_queue_id, binding_id)`, sealed into a
/// pool-backed `Filled` packet. Returns the decryptable packet plus the matching opener.
///
/// The sealer (Client) and opener (Server) are derived from `fake_deterministic` path-secret
/// entries, so they share the same application key and the opener authenticates the sealer's
/// output. The frame's `dest_queue_id`/`binding_id` are carried in the (cleartext, AEAD-AAD)
/// application header — exactly the routing fields the fast path reads to steer dispatch.
fn sealed_queue_msg_packet(
    dest_queue_id: VarInt,
    binding_id: VarInt,
) -> (
    datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
    crate::crypto::awslc::open::Application,
    usize,
) {
    // Geometry consistent with the 17-byte payload: a single-chunk message.
    sealed_queue_msg_packet_geometry(dest_queue_id, binding_id, 17, 17, 0)
}

/// Like [`sealed_queue_msg_packet`] but lets the caller override the message-geometry header
/// fields (`message_size`, `chunk_size`, `chunk_index`). These are cleartext routing/geometry
/// fields carried as AEAD associated data, so an in-flight bit-flip of any of them produces a
/// packet exactly like this — one whose header decodes fine but whose geometry the receiver's
/// `MsgTable::insert` rejects (e.g. `chunk_index` past the implied chunk count → `OffsetOverflow`).
fn sealed_queue_msg_packet_geometry(
    dest_queue_id: VarInt,
    binding_id: VarInt,
    message_size: u64,
    chunk_size: u64,
    chunk_index: u64,
) -> (
    datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
    crate::crypto::awslc::open::Application,
    usize,
) {
    let peer: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let sealer_entry = PathSecretEntry::builder(peer)
        .endpoint_type(endpoint::Type::Client)
        .build();
    let opener_entry = PathSecretEntry::builder(peer)
        .endpoint_type(endpoint::Type::Server)
        .build();

    let key_id = VarInt::ZERO;
    let sealer = sealer_entry.secret().application_sealer(key_id);
    let opener = opener_entry.secret().application_opener(key_id);
    let credentials = crate::credentials::Credentials {
        id: *sealer_entry.secret().id(),
        key_id,
    };

    let payload = b"queue-msg-payload".to_vec();

    let header = Header::QueueMsg {
        queue_pair: QueuePair {
            source_queue_id: VarInt::from_u8(7),
            dest_queue_id,
        },
        binding_id,
        msg_id: VarInt::ZERO,
        stream_offset: VarInt::ZERO,
        largest_offset: VarInt::new(payload.len() as u64).unwrap(),
        message_size: VarInt::new(message_size).unwrap(),
        chunk_size: VarInt::new(chunk_size).unwrap(),
        chunk_index: VarInt::new(chunk_index).unwrap(),
        is_fin: true,
        is_wakeup: true,
        blocked: false,
        dest_acceptor_id: None,
        priority: crate::credit::Priority::default(),
    };

    // Encode the single-frame application header: [Header][payload_len varint].
    let mut header_buf = vec![0u8; header.metadata_len(payload.len())];
    {
        let mut enc = s2n_codec::EncoderBuffer::new(&mut header_buf);
        enc.encode(&header);
        let plen = VarInt::new(payload.len() as u64).unwrap();
        enc.encode(&plen);
    }

    // Seal the datagram into a scratch buffer.
    let mut buf = vec![0u8; 65536];
    let routing_info = datagram::RoutingInfo::SenderId {
        source_sender_id: VarInt::from_u8(7),
    };
    let mut header_reader = &header_buf[..];
    let mut payload_reader = &payload[..];
    let encoded_len = datagram::encoder::encode(
        s2n_codec::EncoderBuffer::new(&mut buf),
        443,
        routing_info,
        Some(VarInt::ZERO),
        VarInt::try_from(header_buf.len() as u64).unwrap(),
        &mut header_reader,
        VarInt::try_from(payload.len() as u64).unwrap(),
        &mut payload_reader,
        &sealer,
        &credentials,
    );
    assert!(encoded_len > 0);

    // Copy the sealed bytes into a pool-backed `Filled` descriptor, mirroring the recv path.
    let pool = pool::Pool::new(u16::MAX);
    let unfilled = pool.alloc::<SyncRecycler>().expect("pool alloc");
    let segments = unfilled
        .fill_with(|addr, _cmsg, mut iov| {
            iov[..encoded_len].copy_from_slice(&buf[..encoded_len]);
            addr.set(peer.into());
            Ok::<_, core::convert::Infallible>(encoded_len)
        })
        .ok()
        .expect("fill_with");
    let mut filled = segments.take_filled();

    // Decode metadata from the filled bytes, then re-attach the storage — same as the router.
    let meta = {
        let decode_buf = DecoderBufferMut::new(&mut filled[..]);
        datagram::decoder::Meta::decode(&decode_buf, (), TAG_LEN).expect("meta decode")
    };
    let packet = meta.with_storage(filled).ok().expect("with_storage");
    let decrypt_len = packet.decrypt_into_len();

    (packet, opener, decrypt_len)
}

/// Build a fresh (no slots bound) Server `QueueView`. Any QueueMsg routed at it rejects with
/// `Unallocated` *before* the scatter-decrypt runs — the un-authenticated reject path.
fn fresh_server_view() -> recv::QueueView {
    let entry = PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap())
        .endpoint_type(endpoint::Type::Server)
        .build();
    match entry.queue_state() {
        crate::path::secret::map::entry::QueueState::Server(state) => {
            recv::QueueView::Server(state.view())
        }
        _ => unreachable!("server entry must have server queue state"),
    }
}

/// A server `QueueView` with the slot at `(queue_id, binding_id)` already bound, so a QueueMsg
/// routed there passes `validate_msg_dispatch` (binding matches) and reaches `MsgTable::insert`
/// — the geometry-validation stage. Used to exercise the live-stream insert-reject path, as
/// opposed to `fresh_server_view`'s unallocated-binding reject.
fn bound_server_view(queue_id: VarInt, binding_id: VarInt) -> recv::QueueView {
    let entry = PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap())
        .endpoint_type(endpoint::Type::Server)
        .build();
    let mut view = match entry.queue_state() {
        crate::path::secret::map::entry::QueueState::Server(state) => {
            recv::QueueView::Server(state.view())
        }
        _ => unreachable!("server entry must have server queue state"),
    };

    let (freed_batch_tx, freed_batch_rx) = crate::queue::freed_batch_channel();
    let mut freed_batch_tx = freed_batch_tx;
    let server = view.as_server_mut().expect("server view");
    let result = server.bind_for_msg(queue_id, binding_id, &entry, &mut freed_batch_tx);
    let crate::queue::BindResult::NewBinding {
        stream, control, ..
    } = result.ok().expect("slot must bind for the test setup")
    else {
        panic!("expected NewBinding for a fresh slot");
    };

    // The receivers (`stream`/`control`) MUST stay alive: dropping them clears the slot's
    // HAS_RECEIVER flag and frees the binding, which would un-bind the slot and make the
    // dispatch below hit the `Unallocated` reject instead of the `MsgTable::insert` reject we
    // want to exercise. The path-secret `entry` likewise keeps the shared `ServerState` (and its
    // page table) alive. Leak all three for the short-lived test rather than threading them out.
    std::mem::forget(stream);
    std::mem::forget(control);
    std::mem::forget(freed_batch_rx);
    std::mem::forget(entry);
    view
}

/// Assembles the auxiliary args `decrypt_fast_path` needs but that play no role in the
/// reject path under test.
struct FastPathHarness {
    acceptor_registry: acceptor::LocalRegistry<Stream>,
    frame_tx: SubmissionSender,
    freed_batch_tx: crate::queue::FreedBatchTx,
    counters: Arc<counters::Dispatch>,
    stream_clock: crate::time::DefaultClock,
    reader_metrics: Arc<crate::stream::metrics::ReaderMetrics>,
    writer_metrics: Arc<crate::stream::metrics::WriterMetrics>,
    send_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    path_entry: Arc<PathSecretEntry>,
    _freed_batch_rx: crate::queue::FreedBatchRx,
}

impl FastPathHarness {
    fn new() -> Self {
        let registry = crate::counter::Registry::default();
        let (frame_tx, _frame_rx) = crate::endpoint::frame::submission_channel(1);
        let (freed_batch_tx, _freed_batch_rx) = crate::queue::freed_batch_channel();
        Self {
            acceptor_registry: acceptor::Registry::<Stream>::new().local(),
            frame_tx,
            freed_batch_tx,
            counters: counters::Dispatch::new(&registry),
            stream_clock: crate::time::DefaultClock::default(),
            reader_metrics: Arc::new(crate::stream::metrics::ReaderMetrics::new(&registry, "rx")),
            writer_metrics: Arc::new(crate::stream::metrics::WriterMetrics::new(&registry, "tx")),
            send_credit_pool: crate::sync::Arc::new(crate::credit::Pool::new(
                crate::credit::Config::default(),
            )),
            recv_credit_pool: crate::sync::Arc::new(crate::credit::Pool::new(
                crate::credit::Config::default(),
            )),
            path_entry: PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap())
                .endpoint_type(endpoint::Type::Server)
                .build(),
            _freed_batch_rx,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        header: Header,
        opener: &crate::crypto::awslc::open::Application,
        packet: &datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
        decrypt_len: usize,
        queue_view: &mut recv::QueueView,
    ) -> Result<AutoWake, FastPathError> {
        decrypt_fast_path(
            header,
            opener,
            packet,
            decrypt_len,
            queue_view,
            &mut self.acceptor_registry,
            &mut self.frame_tx,
            &mut self.freed_batch_tx,
            &self.counters,
            &self.path_entry,
            &self.stream_clock,
            &self.reader_metrics,
            &self.writer_metrics,
            &self.send_credit_pool,
            &self.recv_credit_pool,
        )
    }
}

fn single_queue_msg_header(
    packet: &datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
    decrypt_len: usize,
) -> Header {
    decode::detect_single_queue_msg(packet.application_header(), decrypt_len)
        .expect("packet must be a single QueueMsg frame")
}

/// A QueueMsg routed to a binding the receiver rejects (here: an unallocated queue) takes the
/// fast-path reject branch *before* the scatter-decrypt, so the packet is never authenticated.
/// The fix surfaces this as `FastPathError::AuthForDrop` so the caller authenticates before
/// ACKing — rather than the old behavior of returning `Ok(AutoWake::default())`, which ACKed
/// the packet without ever verifying its AEAD tag.
#[test]
fn fast_path_unallocated_binding_requires_auth_before_ack() {
    let dest_queue_id = VarInt::from_u8(0);
    let binding_id = VarInt::from_u8(1);
    let (packet, opener, decrypt_len) = sealed_queue_msg_packet(dest_queue_id, binding_id);
    let header = single_queue_msg_header(&packet, decrypt_len);

    let mut view = fresh_server_view();
    let mut harness = FastPathHarness::new();

    let result = harness.call(header, &opener, &packet, decrypt_len, &mut view);
    assert!(
        matches!(result, Err(FastPathError::AuthForDrop)),
        "an un-authenticated binding reject must demand authentication before ACK, \
         not report success"
    );
}

/// End-to-end of the closure's `AuthForDrop` recovery: an authentic packet that routes to a
/// reject authenticates in place (→ ACK is safe), while a packet whose AEAD-AAD routing field
/// was corrupted in flight fails authentication (→ no ACK → the genuine packet retransmits).
///
/// This is the behavioral split the bug turned on: pre-fix BOTH cases were ACKed (the corrupted
/// one silently, never authenticated), so a corrupted-routing packet for a *live* stream made
/// the sender free that packet number and stop retransmitting — a permanent stream hole.
#[test]
fn fast_path_auth_for_drop_distinguishes_tampered_packet() {
    // Authentic packet routed to an unallocated queue: in-place auth succeeds.
    {
        let (mut packet, opener, decrypt_len) =
            sealed_queue_msg_packet(VarInt::from_u8(0), VarInt::from_u8(1));
        let header = single_queue_msg_header(&packet, decrypt_len);
        let mut view = fresh_server_view();
        let mut harness = FastPathHarness::new();

        let result = harness.call(header, &opener, &packet, decrypt_len, &mut view);
        assert!(matches!(result, Err(FastPathError::AuthForDrop)));
        // The closure authenticates in place before ACKing; an authentic packet passes.
        assert!(
            packet.decrypt_in_place(&opener).is_ok(),
            "authentic packet must authenticate so the ACK is legitimate"
        );
    }

    // Same scenario, but a byte the AEAD tag authenticates is flipped in flight: the reject
    // path is still taken, yet in-place authentication now fails, so the caller sends no ACK.
    {
        let (mut packet, opener, decrypt_len) =
            sealed_queue_msg_packet(VarInt::from_u8(0), VarInt::from_u8(1));
        let header = single_queue_msg_header(&packet, decrypt_len);
        let mut view = fresh_server_view();
        let mut harness = FastPathHarness::new();

        let result = harness.call(header, &opener, &packet, decrypt_len, &mut view);
        assert!(matches!(result, Err(FastPathError::AuthForDrop)));

        // Corrupt an authenticated byte, then confirm in-place auth fails → ACK suppressed →
        // the genuine packet is retransmitted rather than freed by a spurious ACK.
        packet.payload_mut()[0] ^= 0xFF;
        assert!(
            packet.decrypt_in_place(&opener).is_err(),
            "a tampered packet must fail authentication so its ACK is suppressed and the \
             genuine packet is retransmitted"
        );
    }
}

/// REPRO (fails on HEAD): a QueueMsg whose AEAD-authenticated *geometry* fields were corrupted in
/// flight routes to a LIVE, bound slot, passes `validate_msg_dispatch`, then is rejected by
/// `MsgTable::insert` (here `chunk_index` past the implied chunk count → `OffsetOverflow`). The
/// fast-path scatter-decrypt — the ONLY place the AEAD tag is checked on this path — never runs,
/// because `Slot::push_msg` swallows the insert error as `Ok((AutoWake::default(), 0))` BEFORE
/// invoking the write callback.
///
/// `decrypt_fast_path` therefore returns `Ok(AutoWake::default())` and `process` ACKs an
/// un-authenticated packet. This is the same vulnerability class the `AuthForDrop` fix closed for
/// the binding-reject path (`MsgError::Queue`), but the `MsgTable::insert` rejections take a
/// different return (`Ok`, not `MsgError::Queue`) and were missed. The consequence is identical:
/// corrupting an in-flight chunk's geometry makes the sender free that packet number and stop
/// retransmitting → permanent stream hole on a live stream.
///
/// The fix should route insert-rejections that fired before authentication through
/// `FastPathError::AuthForDrop` as well, so the closure authenticates in place and suppresses the
/// ACK on a tampered packet.
#[test]
fn fast_path_insert_reject_requires_auth_before_ack() {
    let dest_queue_id = VarInt::from_u8(0);
    let binding_id = VarInt::from_u8(1);

    // message_size=17, chunk_size=17 → exactly 1 chunk (index 0). A corrupted chunk_index of 5
    // is past that count, so MsgTable::insert returns OffsetOverflow.
    let (packet, opener, decrypt_len) =
        sealed_queue_msg_packet_geometry(dest_queue_id, binding_id, 17, 17, 5);
    let header = single_queue_msg_header(&packet, decrypt_len);

    let mut view = bound_server_view(dest_queue_id, binding_id);
    // Sanity: confirm the slot is actually bound, so we exercise the `MsgTable::insert` reject
    // path and NOT the unallocated-binding reject (which returns AuthForDrop for a different
    // reason — without this guard the test could pass even if the binding silently failed).
    {
        let server = view.as_server_mut().unwrap();
        // Use a non-front msg_id (10) so the message completes but does NOT drain (the table
        // front is still the never-sent msg_id 0), leaving `base_id == 0`. That keeps msg_id 0
        // fresh for the corrupted packet below, so it hits `OffsetOverflow` (a fresh entry with
        // 1 chunk vs chunk_index 5) rather than `Stale`.
        let probe = server.send_msg::<core::convert::Infallible>(
            dest_queue_id,
            binding_id,
            10, // msg_id (non-front, stays buffered)
            0,  // stream_offset
            17, // peer_max_offset
            17, // message_size
            17, // chunk_size
            0,  // chunk_index (valid: 1-chunk message)
            17, // payload_len
            false,
            false,
            false,
            |ptr, len| {
                unsafe { core::ptr::write_bytes(ptr, 0, len as usize) };
                Ok(())
            },
        );
        assert!(
            probe.is_ok(),
            "setup: a VALID-geometry QueueMsg to this slot must succeed (Ok), proving the slot is \
             bound; if this were an Unallocated error the test below would pass for the wrong reason"
        );
    }
    let mut harness = FastPathHarness::new();

    let result = harness.call(header, &opener, &packet, decrypt_len, &mut view);
    assert!(
        matches!(result, Err(FastPathError::AuthForDrop)),
        "a QueueMsg rejected by MsgTable::insert BEFORE the scatter-decrypt was never \
         authenticated, so the fast path must demand auth-before-ACK (AuthForDrop) rather than \
         report success and let `process` ACK an un-verified packet — got {}",
        match result {
            Ok(_) => "Ok (packet would be ACKed un-authenticated — BUG)",
            Err(FastPathError::HeaderMismatch) => "HeaderMismatch",
            Err(FastPathError::WriteFailed) => "WriteFailed",
            Err(FastPathError::AuthForDrop) => "AuthForDrop",
        }
    );
}

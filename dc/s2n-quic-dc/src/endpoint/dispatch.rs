// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Inbound packet processing: decrypt, deduplicate, and dispatch frames to flow queues.
//!
//! A single received packet may contain multiple frames (the frame aggregation model).
//! After decryption and packet-number deduplication, we iterate the frame metadata region
//! and dispatch each frame to its appropriate handler based on the frame header type.

use crate::{
    acceptor,
    byte_vec::ByteVec,
    credentials::Credentials,
    endpoint::{
        counters, decode, error,
        frame::{Frame, Header, SubmissionSender, DEFAULT_TTL},
        id::LocalSenderId,
        msg, recv, routing,
    },
    intrusive::Entry,
    packet::{
        self,
        datagram::{QueuePair, ResetTarget, RoutingInfo},
    },
    path::secret::{map::Entry as PathSecretEntry, Map as PathSecretMap},
    queue::AutoWake,
    socket::{channel, pool::descriptor},
    stream::{PendingValidation, Reader, Stream, Writer},
    tracing::*,
};
use bytes::BytesMut;
use core::time::Duration;
use s2n_quic_core::varint::VarInt;
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub(crate) enum Error {
    PeerStateLookup {
        dest_addr: crate::msg::addr::Addr,
        credentials: Credentials,
        control_out: Vec<u8>,
    },
    Decryption {
        credentials: Credentials,
        packet_number: VarInt,
    },
    Duplicate {
        credentials: Credentials,
        packet_number: VarInt,
    },
    /// `check_dedup` detected that the key-id was already registered (definite replay)
    /// or outside the replay window (possible replay / too old).  The peer should be
    /// notified to trigger a re-handshake.
    StaleKey {
        dest_addr: crate::msg::addr::Addr,
        credentials: Credentials,
        packet_number: VarInt,
        control_out: Vec<u8>,
    },
    MissingSenderId,
}

/// Process a received datagram packet.
///
/// Authenticates (decrypt), deduplicates by packet number, updates ACK state, then
/// dispatches each frame in the packet to its type-specific handler. Response frames
/// (ACKs, QueueValidateRequest, QueueReset) are emitted to `response_tx`.
pub(crate) fn process<Clk, Route>(
    packet: Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
    recv_cache: &mut recv::Cache,
    ack_burst_tx: &mut impl channel::UnboundedSender<Rc<RefCell<recv::Context>>>,
    idle_wheel_tx: &mut impl channel::UnboundedSender<Rc<RefCell<recv::Context>>>,
    path_secret_map: &PathSecretMap,
    acceptor_registry: &mut acceptor::LocalRegistry<PendingValidation>,
    frame_tx: &mut SubmissionSender,
    sender_tx: &mut impl channel::UnboundedSender<Entry<msg::Sender>>,
    freed_batch_tx: &mut crate::queue::FreedBatchTx,
    clock: &Clk,
    counters: &counters::Dispatch,
    route: &Route,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) -> Result<(), Error>
where
    Clk: s2n_quic_core::time::Clock + crate::time::precision::Clock + ?Sized,
    Route: routing::SenderRoute,
{
    let credentials = *packet.credentials();
    let packet_number = packet.packet_number();
    let routing_info = packet.routing_info();

    let source_sender_id = match routing_info {
        RoutingInfo::SenderId { source_sender_id } => source_sender_id,
        RoutingInfo::None => return Err(Error::MissingSenderId),
    };

    // Collect the fields we need before the closure captures `packet`.
    let app_header_slice: &[u8] = packet.application_header();
    let decrypt_len = packet.decrypt_into_len();
    let ecn = packet.storage().ecn();

    // Get or create peer receive state, decrypting the packet on-demand.
    //
    // The decrypt closure is invoked with the opener (cached on hit, freshly derived
    // on miss).  `post_authentication` is called inside `get_or_insert` only on a
    // cache miss — recording the key-id in the receiver's replay window for the first
    // packet of a new session.  Cache hits skip `post_authentication` because many
    // packets legitimately share the same key-id within a session; per-packet replay
    // protection is handled by `dedup_filter` inside the `Context`.  On a cache miss
    // the Context is inserted only once both decrypt and `post_authentication` succeed,
    // preventing stale path-secret entries from poisoning the cache.
    let mut control_out = Vec::new();
    let decrypt_fn = |opener: &crate::crypto::awslc::open::Application| {
        let _guard = counters.rx_decrypt_time.start();
        let mut buf = BytesMut::with_capacity(decrypt_len);
        let written = packet
            .decrypt_into(opener, bytes::BufMut::chunk_mut(&mut buf))
            .map_err(|err| {
                warn!(
                    %credentials,
                    packet_number = packet_number.as_u64(),
                    error = %err,
                    "decrypt_into failed"
                );
            })
            .ok()?;
        if written != decrypt_len {
            warn!(
                %credentials,
                packet_number = packet_number.as_u64(),
                expected_len = decrypt_len,
                actual_len = written,
                "decrypt_into wrote an unexpected number of bytes"
            );
            return None;
        }
        // SAFETY: `buf` was allocated with `with_capacity(decrypt_len)` and
        // `chunk_mut` exposed exactly that region to `decrypt_into`, which
        // initialized `decrypt_len` bytes.  We returned early unless
        // `written == decrypt_len`.
        unsafe { buf.set_len(decrypt_len) };
        Some(buf)
    };
    let (decrypted, peer_rc, cache_hit) = {
        let _guard = counters.rx_peer_lookup_time.start();
        let remote_addr = packet.storage().remote_address().get();
        match recv_cache.get_or_insert(
            &credentials,
            crate::endpoint::id::RemoteSenderId::new(source_sender_id),
            path_secret_map,
            clock,
            remote_addr,
            &mut control_out,
            route,
            decrypt_fn,
        ) {
            Ok(v) => v,
            Err(recv::CacheError::PathSecretNotFound) => {
                let mut dest_addr = crate::msg::addr::Addr::new(remote_addr);
                dest_addr.set_port(packet.source_control_port());
                return Err(Error::PeerStateLookup {
                    dest_addr,
                    credentials,
                    control_out,
                });
            }
            Err(recv::CacheError::DecryptFailed) => {
                warn!(
                    %credentials,
                    packet_number = packet_number.as_u64(),
                    "failed to decrypt packet"
                );
                return Err(Error::Decryption {
                    credentials,
                    packet_number,
                });
            }
            Err(recv::CacheError::ReplayDetected) => {
                let mut dest_addr = crate::msg::addr::Addr::new(remote_addr);
                dest_addr.set_port(packet.source_control_port());
                return Err(Error::StaleKey {
                    dest_addr,
                    credentials,
                    packet_number,
                    control_out,
                });
            }
        }
    };
    if cache_hit {
        counters.rx_peer_cache_hit.add(1);
    } else {
        counters.rx_peer_cache_miss.add(1);
        let _ = idle_wheel_tx.send(peer_rc.clone());
    }
    let mut peer = peer_rc.borrow_mut();

    // Packet number deduplication
    if peer.dedup_filter.on_packet_number(packet_number).is_err() {
        return Err(Error::Duplicate {
            credentials,
            packet_number,
        });
    }

    // Update activity tracking on the shared PathSecretEntry
    peer.path_entry
        .touch_activity(crate::time::precision::Clock::now(clock));
    peer.ecn_counts.increment(ecn);
    counters.on_ecn(ecn);
    let now = clock.get_time();
    peer.ack_ranges.on_packet_received(packet_number, now);

    counters.rx_packet_size.record_value(decrypt_len as u64);

    let mut payload_storage = decrypted;

    // Multi-frame packet: `app_header_slice` contains the per-frame metadata
    // (Header type tag + optional payload_len VarInt) and `payload_storage`
    // contains the concatenated, decrypted frame payloads.

    let _dispatch_guard = counters.rx_dispatch_time.start();
    let mut is_ack_eliciting = false;
    let mut frame_count = 0u64;
    for result in decode::decode_frames(app_header_slice) {
        match result {
            Ok((header, frame_payload_len)) => {
                frame_count += 1;
                counters.on_received_frame(&header);
                // Validate that the claimed payload length fits within the
                // remaining payload storage.
                if frame_payload_len > payload_storage.len() {
                    warn!(
                        %credentials,
                        packet_number = packet_number.as_u64(),
                        frame_payload_len,
                        remaining = payload_storage.len(),
                        "frame payload length exceeds remaining packet payload"
                    );
                    break;
                }

                if header.is_ack_eliciting() {
                    is_ack_eliciting = true;
                }

                // Split the frame's payload out of the shared storage.
                let frame_payload = payload_storage.split_to(frame_payload_len);
                dispatch_decoded_frame(
                    header,
                    source_sender_id,
                    frame_payload,
                    &mut peer,
                    &credentials,
                    acceptor_registry,
                    frame_tx,
                    freed_batch_tx,
                    sender_tx,
                    counters,
                    waker_sink,
                );
            }
            Err(err) => {
                warn!(
                    %credentials,
                    packet_number = packet_number.as_u64(),
                    ?err,
                    "failed to decode multi-frame packet metadata"
                );
                break;
            }
        }
    }

    if !payload_storage.is_empty() {
        warn!(
            %credentials,
            packet_number = packet_number.as_u64(),
            remaining = payload_storage.len(),
            "multi-frame packet has unconsumed payload bytes"
        );
    }

    counters.rx_frames_per_packet.record_value(frame_count);

    let mut enqueue_pending_ack = false;
    if is_ack_eliciting {
        match peer.ack_state.on_ack_eliciting() {
            Ok(()) | Err(s2n_quic_core::state::Error::NoOp { .. }) => {}
            Err(s2n_quic_core::state::Error::InvalidTransition { .. }) => {
                counters.rx_ack_state_impossible.add(1);
                debug_assert!(false, "on_ack_eliciting transition failed");
            }
        }

        // Only enqueue into the burst queue when the state is Scheduled.
        // When FlushedStale, the ack_completion_task handles re-encoding after
        // the in-flight ACK completes — enqueueing here would leave a stale link
        // that outlives the Scheduled state.
        if !peer.ack_burst.is_linked() && peer.ack_state.is_scheduled() {
            enqueue_pending_ack = true;
        }
    }
    peer.invariants();
    drop(peer);

    if enqueue_pending_ack {
        let _ = ack_burst_tx.send(peer_rc);
    }

    Ok(())
}

// ── Multi-frame dispatch ───────────────────────────────────────────────────

/// Dispatch a single frame decoded from a multi-frame `SenderId` packet.
///
/// This routes each decoded frame to the same handler as its single-frame
/// `RoutingInfo` counterpart, using the packet-level `source_sender_id` for
/// frame types that require it (e.g., QueueInit).
#[allow(clippy::too_many_arguments)]
fn dispatch_decoded_frame(
    header: Header,
    source_sender_id: VarInt,
    payload: BytesMut,
    peer: &mut recv::Context,
    credentials: &Credentials,
    acceptor_registry: &mut acceptor::LocalRegistry<PendingValidation>,
    frame_tx: &mut SubmissionSender,
    freed_batch_tx: &mut crate::queue::FreedBatchTx,
    sender_tx: &mut impl channel::UnboundedSender<Entry<msg::Sender>>,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    match header {
        Header::QueueInit { .. } => {
            todo!("remove QueueInit path")
        }
        Header::QueueValidateRequest { .. } => {
            todo!("remove QueueValidateRequest path")
        }
        Header::QueueInitValidate { .. } => {
            todo!("remove QueueInitValidate path")
        }
        Header::QueueData {
            queue_pair,
            binding_id,
            offset,
            is_fin,
            dest_acceptor_id,
        } => {
            if let Some(acceptor_id) = dest_acceptor_id {
                handle_queue_data_init(
                    peer,
                    queue_pair,
                    binding_id,
                    offset,
                    is_fin,
                    acceptor_id,
                    payload,
                    acceptor_registry,
                    frame_tx,
                    freed_batch_tx,
                    counters,
                    waker_sink,
                );
            } else {
                handle_queue_data(
                    peer, queue_pair, binding_id, offset, is_fin, payload, counters, waker_sink,
                );
            }
        }
        Header::QueueControl {
            queue_pair,
            binding_id,
        } => {
            handle_queue_control(peer, queue_pair, binding_id, payload, counters, waker_sink);
        }
        Header::QueueMaxData {
            queue_pair,
            binding_id,
            maximum_data,
        } => {
            handle_queue_max_data(
                peer,
                queue_pair,
                binding_id,
                maximum_data,
                counters,
                waker_sink,
            );
        }
        Header::QueueReset {
            dest_queue_id,
            binding_id,
            reset_target,
            error_code,
            dest_acceptor_id: _,
        } => {
            handle_queue_reset(
                peer,
                dest_queue_id,
                binding_id,
                reset_target,
                error_code,
                counters,
                waker_sink,
            );
        }
        Header::QueueInitReset { .. } => {
            todo!("remove QueueInitReset path")
        }
        Header::QueueInitFin { .. } => {
            todo!("remove QueueInitFin path")
        }
        Header::QueueFree {
            free_request_id,
            smallest_queue_id,
        } => {
            handle_queue_free(
                peer,
                free_request_id,
                smallest_queue_id,
                payload,
                counters,
                waker_sink,
            );
        }
        Header::Ack {
            dest_sender_id,
            ack_delay: ack_delay_micros,
            ..
        } => {
            let ack_delay = Duration::from_micros(ack_delay_micros.as_u64());
            let message = msg::Sender::ReceivedAck {
                local_sender_id: crate::endpoint::id::LocalSenderId::new(dest_sender_id),
                path_secret_entry: peer.path_entry.clone(),
                payload,
                ack_delay,
            };
            if sender_tx.send(Entry::new(message)).is_err() {
                warn!(
                    %credentials,
                    source_sender_id = source_sender_id.as_u64(),
                    dest_sender_id = dest_sender_id.as_u64(),
                    "dropping ACK sender message; sender queue is closed"
                );
            }
        }
    }
}

// ── QueueData ──────────────────────────────────────────────────────────────

fn handle_queue_free(
    peer: &mut recv::Context,
    free_request_id: VarInt,
    smallest_queue_id: VarInt,
    payload: BytesMut,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let Some(client_view) = peer.queue_view.as_client_mut() else {
        debug!("QueueFree received on server context - dropping");
        return;
    };

    let decoder = DeltaDecoder::new(smallest_queue_id, &payload);
    let result = client_view.free(free_request_id, decoder, &mut |w| {
        let _ = waker_sink.send(AutoWake::new(Some(w)));
    });
    counters
        .rx_queue_free_slots
        .record_value(result.slots as u64);
    counters
        .rx_queue_free_ranges
        .record_value(result.ranges as u64);
}

struct DeltaDecoder<'a> {
    pending: Option<VarInt>,
    payload: s2n_codec::DecoderBuffer<'a>,
}

impl<'a> DeltaDecoder<'a> {
    fn new(smallest_queue_id: VarInt, payload: &'a [u8]) -> Self {
        Self {
            pending: Some(smallest_queue_id),
            payload: s2n_codec::DecoderBuffer::new(payload),
        }
    }
}

impl Iterator for DeltaDecoder<'_> {
    type Item = Result<core::ops::RangeInclusive<VarInt>, s2n_codec::DecoderError>;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.pending.take()?;
        let mut end = start;

        loop {
            if self.payload.is_empty() {
                return Some(Ok(start..=end));
            }
            let (delta, buffer) = match self.payload.decode::<VarInt>() {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let next_id = match end.checked_add(delta + VarInt::from_u8(1)) {
                Some(v) => v,
                None => {
                    return Some(Err(s2n_codec::DecoderError::InvariantViolation(
                        "delta overflow",
                    )))
                }
            };
            self.payload = buffer;
            if delta == VarInt::ZERO {
                end = next_id;
            } else {
                self.pending = Some(next_id);
                return Some(Ok(start..=end));
            }
        }
    }
}

fn handle_queue_data(
    peer: &mut recv::Context,
    queue_pair: QueuePair,
    binding_id: VarInt,
    offset: VarInt,
    is_fin: bool,
    buf: BytesMut,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = queue_pair.dest_queue_id;
    let payload_len = buf.len();
    let entry = msg::Stream::Data {
        offset,
        fin: is_fin,
        payload: buf,
    }
    .into();

    match peer
        .queue_view
        .send_stream(local_queue_id, binding_id, entry)
    {
        Ok(waker) => {
            let _ = waker_sink.send(waker);
            counters.rx_data_ok.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                offset = offset.as_u64(),
                payload_len,
                is_fin,
                "QueueData dispatched"
            );
        }
        Err(crate::queue::Error::Unallocated(_)) => {
            counters.rx_data_unallocated.add(1);
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueData for unallocated queue - dropping"
            );
        }
        Err(crate::queue::Error::HalfClosed(_)) => {
            counters.rx_data_half_closed.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueData for half-closed stream - dropping"
            );
        }
        Err(crate::queue::Error::StaleBinding(_)) => {
            counters.rx_data_stale_binding.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueData stale binding - dropping"
            );
        }
        Err(crate::queue::Error::FutureBinding(_)) => {
            counters.rx_data_future_binding.add(1);
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueData future binding - dropping"
            );
        }
        Err(crate::queue::Error::SenderClosed) => {
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueData for closed sender - dropping"
            );
        }
        Err(crate::queue::Error::CapExceeded(_)) => {
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueData queue_id exceeds cap - dropping"
            );
        }
    }
}

// ── QueueData Init ─────────────────────────────────────────────────────────

fn handle_queue_data_init(
    peer: &mut recv::Context,
    queue_pair: QueuePair,
    binding_id: VarInt,
    offset: VarInt,
    is_fin: bool,
    acceptor_id: VarInt,
    buf: BytesMut,
    acceptor_registry: &mut acceptor::LocalRegistry<PendingValidation>,
    frame_tx: &mut SubmissionSender,
    freed_batch_tx: &mut crate::queue::FreedBatchTx,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let Some(server_view) = peer.queue_view.as_server_mut() else {
        error!(
            binding_id = binding_id.as_u64(),
            queue_id = queue_pair.dest_queue_id.as_u64(),
            "QueueData init received on client context - dropping"
        );
        return;
    };

    let Some(acceptor_sender) = acceptor_registry.get(acceptor_id) else {
        counters.rx_init_no_acceptor.add(1);
        debug!(
            binding_id = binding_id.as_u64(),
            acceptor_id = acceptor_id.as_u64(),
            "QueueData init rejected - acceptor not found, sending reset"
        );
        server_view.record_freed(queue_pair.dest_queue_id, &peer.path_entry, freed_batch_tx);
        send_reset(
            &peer.path_entry,
            queue_pair.source_queue_id,
            binding_id,
            error::ACCEPTOR_NOT_FOUND,
            frame_tx,
        );
        return;
    };

    let entry = msg::Stream::Data {
        offset,
        fin: is_fin,
        payload: buf,
    }
    .into();

    match server_view.bind_and_send_stream(
        queue_pair.dest_queue_id,
        binding_id,
        entry,
        &peer.path_entry,
        freed_batch_tx,
    ) {
        Ok(crate::queue::BindResult::NewBinding {
            waker,
            stream,
            control,
        }) => {
            let writer = Writer::new_server(
                frame_tx.clone(),
                peer.path_entry.clone(),
                queue_pair.source_queue_id,
                acceptor_id,
                control,
            );
            let peer_fin = is_fin;
            let reader = Reader::new_server(
                frame_tx.clone(),
                peer.path_entry.clone(),
                queue_pair.source_queue_id,
                stream,
                peer_fin,
            );
            let pending = PendingValidation::new(Stream::new(reader, writer));

            match acceptor_sender.send(pending) {
                Ok((mut evicted, acceptor_waker)) => {
                    if let Some(ref mut ev) = evicted {
                        ev.reset(crate::stream::endpoint::Error::ServerBusy);
                    }
                    counters.queue_accepted.add(1);
                    let _ = waker_sink.send(AutoWake::new(acceptor_waker));
                }
                Err(acceptor::channel::SendError::Closed(mut stream)) => {
                    stream.disable();
                    counters.rx_init_acceptor_closed.add(1);
                }
                Err(acceptor::channel::SendError::NoSlots(mut stream)) => {
                    stream.disable();
                    counters.rx_init_acceptor_no_slots.add(1);
                }
            }

            let _ = waker_sink.send(waker);

            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = queue_pair.dest_queue_id.as_u64(),
                acceptor_id = acceptor_id.as_u64(),
                "QueueData init - new binding created"
            );
        }
        Ok(crate::queue::BindResult::Bound(waker)) => {
            let _ = waker_sink.send(waker);
            counters.rx_data_ok.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = queue_pair.dest_queue_id.as_u64(),
                "QueueData init - pushed to existing binding"
            );
        }
        Err(_) => {
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = queue_pair.dest_queue_id.as_u64(),
                "QueueData init bind failed - dropping"
            );
        }
    }
}

fn send_reset(
    path_secret_entry: &Arc<PathSecretEntry>,
    dest_queue_id: VarInt,
    binding_id: VarInt,
    error_code: VarInt,
    frame_tx: &mut SubmissionSender,
) {
    let frame = Frame {
        source_sender_id: LocalSenderId::UNSPECIFIED,
        header: Header::QueueReset {
            dest_queue_id,
            binding_id,
            reset_target: ResetTarget::Both,
            error_code,
            dest_acceptor_id: None,
        },
        payload: ByteVec::new(),
        path_secret_entry: path_secret_entry.clone(),
        completion: None,
        status: crate::endpoint::frame::TransmissionStatus::default(),
        ttl: DEFAULT_TTL,
        transmission_time: None,
    };
    let _ = frame_tx.send_batch(Entry::new(frame));
}

// ── QueueControl ───────────────────────────────────────────────────────────

fn handle_queue_control(
    peer: &mut recv::Context,
    queue_pair: QueuePair,
    binding_id: VarInt,
    buf: BytesMut,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = queue_pair.dest_queue_id;
    let payload_len = buf.len();
    let entry = msg::Control::Frames { payload: buf }.into();

    match peer
        .queue_view
        .send_control(local_queue_id, binding_id, entry)
    {
        Ok(waker) => {
            let _ = waker_sink.send(waker);
            counters.rx_queue_control_ok.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                payload_len,
                "QueueControl dispatched"
            );
        }
        Err(crate::queue::Error::Unallocated(_)) => {
            counters.rx_queue_control_unallocated.add(1);
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueControl for unallocated queue - dropping"
            );
        }
        Err(crate::queue::Error::HalfClosed(_)) => {
            counters.rx_queue_control_half_closed.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueControl for half-closed queue - dropping"
            );
        }
        Err(_) => {
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueControl dispatch error - dropping"
            );
        }
    }
}

// ── QueueMaxData ───────────────────────────────────────────────────────────

fn handle_queue_max_data(
    peer: &mut recv::Context,
    queue_pair: QueuePair,
    binding_id: VarInt,
    maximum_data: VarInt,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = queue_pair.dest_queue_id;
    let entry = msg::Control::MaxData { maximum_data }.into();

    match peer
        .queue_view
        .send_control(local_queue_id, binding_id, entry)
    {
        Ok(waker) => {
            let _ = waker_sink.send(waker);
            counters.rx_queue_control_ok.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                maximum_data = maximum_data.as_u64(),
                "QueueMaxData dispatched"
            );
        }
        Err(crate::queue::Error::Unallocated(_)) => {
            counters.rx_queue_control_unallocated.add(1);
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueMaxData for unallocated queue - dropping"
            );
        }
        Err(crate::queue::Error::HalfClosed(_)) => {
            counters.rx_queue_control_half_closed.add(1);
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueMaxData for half-closed queue - dropping"
            );
        }
        Err(_) => {
            trace!(
                binding_id = binding_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "QueueMaxData dispatch error - dropping"
            );
        }
    }
}

// ── QueueReset ─────────────────────────────────────────────────────────────

fn handle_queue_reset(
    peer: &mut recv::Context,
    dest_queue_id: VarInt,
    binding_id: VarInt,
    reset_target: ResetTarget,
    error_code: VarInt,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    match reset_target {
        ResetTarget::Both => {
            counters.rx_reset_both.add(1);
            let stream_entry = msg::Stream::Reset { error_code }.into();
            let control_entry = msg::Control::Reset { error_code }.into();
            if let Ok(waker) = peer
                .queue_view
                .send_stream(dest_queue_id, binding_id, stream_entry)
            {
                let _ = waker_sink.send(waker);
            }
            if let Ok(waker) =
                peer.queue_view
                    .send_control(dest_queue_id, binding_id, control_entry)
            {
                let _ = waker_sink.send(waker);
            }
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = dest_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "QueueReset(Both) dispatched"
            );
        }
        ResetTarget::Stream => {
            counters.rx_reset_stream.add(1);
            let stream_entry = msg::Stream::Reset { error_code }.into();
            if let Ok(waker) = peer
                .queue_view
                .send_stream(dest_queue_id, binding_id, stream_entry)
            {
                let _ = waker_sink.send(waker);
            }
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = dest_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "QueueReset(Stream) dispatched"
            );
        }
        ResetTarget::Control => {
            counters.rx_reset_control.add(1);
            let control_entry = msg::Control::Reset { error_code }.into();
            if let Ok(waker) =
                peer.queue_view
                    .send_control(dest_queue_id, binding_id, control_entry)
            {
                let _ = waker_sink.send(waker);
            }
            debug!(
                binding_id = binding_id.as_u64(),
                queue_id = dest_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "QueueReset(Control) dispatched"
            );
        }
    }
}

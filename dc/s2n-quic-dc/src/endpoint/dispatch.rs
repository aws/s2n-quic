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
        frame::{Frame, Header, PriorityInput, SubmissionSender, DEFAULT_TTL},
        msg,
        recv::{self, AttemptDedupError},
        routing,
    },
    flow,
    flow::queue::AutoWake,
    intrusive::Entry,
    packet::{
        self,
        datagram::{QueuePair, ResetTarget, RoutingInfo},
    },
    path::secret::{map::Entry as PathSecretEntry, Map as PathSecretMap},
    socket::{channel, pool::descriptor},
    stream::{PendingValidation, Reader, Stream, Writer},
};
use bytes::BytesMut;
use core::time::Duration;
use s2n_quic_core::varint::VarInt;
use std::{cell::RefCell, rc::Rc};

const UNSET_SOURCE_SENDER_ID: VarInt = VarInt::MAX;

pub(crate) enum Error {
    PeerStateLookup {
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
    MissingSenderId,
}

/// Process a received datagram packet.
///
/// Authenticates (decrypt), deduplicates by packet number, updates ACK state, then
/// dispatches each frame in the packet to its type-specific handler. Response frames
/// (ACKs, FlowValidateRequest, FlowReset) are emitted to `response_tx`.
pub(crate) fn process<Clk, Route>(
    packet: Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
    recv_cache: &mut recv::Cache,
    ack_burst_tx: &mut impl channel::UnboundedSender<Rc<RefCell<recv::Context>>>,
    idle_wheel_tx: &mut impl channel::UnboundedSender<Rc<RefCell<recv::Context>>>,
    path_secret_map: &PathSecretMap,
    acceptor_registry: &acceptor::Registry<PendingValidation>,
    frame_tx: &SubmissionSender,
    response_tx: &mut impl channel::UnboundedSender<PriorityInput>,
    sender_tx: &mut impl channel::UnboundedSender<Entry<msg::Sender>>,
    queue_dispatcher: &mut msg::queue::Dispatcher,
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

    // Get or create peer receive state
    let mut control_out = Vec::new();
    let (peer_rc, cache_hit) = {
        let _guard = counters.rx_peer_lookup_time.start();
        match recv_cache.get_or_insert(
            &credentials,
            source_sender_id,
            path_secret_map,
            clock,
            &mut control_out,
            route,
        ) {
            Some(v) => v,
            None => {
                return Err(Error::PeerStateLookup {
                    credentials,
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

    // Collect information about the packet layout before decryption.
    let app_header_slice: &[u8] = packet.application_header();
    let decrypt_len = packet.decrypt_into_len();
    let ecn = packet.storage().ecn();

    // Decrypt payload bytes into a BytesMut buffer.
    let mut decrypted = BytesMut::with_capacity(decrypt_len);
    let written = {
        let _guard = counters.rx_decrypt_time.start();
        packet
            .decrypt_into(&peer.opener, bytes::BufMut::chunk_mut(&mut decrypted))
            .map_err(|_| Error::Decryption {
                credentials,
                packet_number,
            })?
    };
    if written != decrypt_len {
        tracing::warn!(
            %credentials,
            packet_number = packet_number.as_u64(),
            expected_len = decrypt_len,
            actual_len = written,
            "decrypt_into wrote an unexpected number of bytes"
        );
        return Err(Error::Decryption {
            credentials,
            packet_number,
        });
    }
    unsafe {
        // SAFETY: `decrypted` was allocated with `with_capacity(decrypt_len)` and `chunk_mut`
        // exposed exactly that uninitialized region to decrypt_into, which initialized
        // `decrypt_len` bytes. We returned early unless `written == decrypt_len`.
        decrypted.set_len(decrypt_len);
    }

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

    let mut response_frames = PriorityInput::default();

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
                    tracing::warn!(
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
                    queue_dispatcher,
                    sender_tx,
                    counters,
                    &mut response_frames,
                    waker_sink,
                );
            }
            Err(err) => {
                tracing::warn!(
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
        tracing::warn!(
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

    let _ = response_tx.send(response_frames);
    Ok(())
}

// ── Multi-frame dispatch ───────────────────────────────────────────────────

/// Dispatch a single frame decoded from a multi-frame `SenderId` packet.
///
/// This routes each decoded frame to the same handler as its single-frame
/// `RoutingInfo` counterpart, using the packet-level `source_sender_id` for
/// frame types that require it (e.g., FlowInit).
#[allow(clippy::too_many_arguments)]
fn dispatch_decoded_frame(
    header: Header,
    source_sender_id: VarInt,
    payload: BytesMut,
    peer: &mut recv::Context,
    credentials: &Credentials,
    acceptor_registry: &acceptor::Registry<PendingValidation>,
    frame_tx: &SubmissionSender,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    sender_tx: &mut impl channel::UnboundedSender<Entry<msg::Sender>>,
    counters: &counters::Dispatch,
    response_frames: &mut PriorityInput,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    match header {
        Header::FlowInit {
            source_queue_id,
            dest_acceptor_id,
            attempt_id,
            stream_id,
            is_fin,
        } => {
            handle_flow_init(
                peer,
                credentials,
                source_sender_id,
                source_queue_id,
                dest_acceptor_id,
                attempt_id,
                stream_id,
                is_fin,
                payload,
                acceptor_registry,
                frame_tx,
                queue_dispatcher,
                counters,
                response_frames,
                waker_sink,
            );
        }
        Header::FlowValidateRequest {
            dest_sender_id,
            queue_pair,
            attempt_id,
            stream_id,
        } => {
            handle_flow_validate_request(
                &peer.path_entry,
                credentials,
                dest_sender_id,
                queue_pair,
                attempt_id,
                stream_id,
                queue_dispatcher,
                counters,
                response_frames,
            );
        }
        Header::FlowInitValidate {
            queue_pair,
            attempt_id,
            stream_id,
        } => {
            handle_flow_init_validate(
                &peer.path_entry,
                credentials,
                queue_pair,
                attempt_id,
                stream_id,
                queue_dispatcher,
                counters,
                response_frames,
                waker_sink,
            );
        }
        Header::FlowData {
            queue_pair,
            stream_id,
            offset,
            is_fin,
        } => {
            handle_flow_data(
                &peer.path_entry,
                credentials,
                queue_pair,
                stream_id,
                offset,
                is_fin,
                payload,
                queue_dispatcher,
                counters,
                response_frames,
                waker_sink,
            );
        }
        Header::FlowControl {
            queue_pair,
            stream_id,
        } => {
            handle_flow_control(
                &peer.path_entry,
                credentials,
                queue_pair,
                stream_id,
                payload,
                queue_dispatcher,
                counters,
                response_frames,
                waker_sink,
            );
        }
        Header::FlowReset {
            dest_queue_id,
            stream_id,
            reset_target,
            error_code,
        } => {
            handle_flow_reset(
                credentials,
                dest_queue_id,
                stream_id,
                reset_target,
                error_code,
                queue_dispatcher,
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
                local_sender_id: dest_sender_id,
                path_secret_entry: peer.path_entry.clone(),
                payload,
                ack_delay,
            };
            if sender_tx.send(Entry::new(message)).is_err() {
                tracing::warn!(
                    %credentials,
                    source_sender_id = source_sender_id.as_u64(),
                    dest_sender_id = dest_sender_id.as_u64(),
                    "dropping ACK sender message; sender queue is closed"
                );
            }
        }
    }
}

fn handle_flow_init(
    peer: &mut recv::Context,
    credentials: &Credentials,
    source_sender_id: VarInt,
    peer_queue_id: VarInt,
    acceptor_id: VarInt,
    attempt_id: VarInt,
    stream_id: VarInt,
    is_fin: bool,
    buf: BytesMut,
    acceptor_registry: &acceptor::Registry<PendingValidation>,
    frame_tx: &SubmissionSender,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    response_frames: &mut PriorityInput,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let create_queue = |handle| {
        let (queue_control, queue_stream) =
            queue_dispatcher.alloc_or_grow(handle, Some(peer_queue_id));
        (queue_control.queue_id(), (queue_control, queue_stream))
    };

    let mut initial_payload: Option<BytesMut> = Some(buf);
    let mut create_stream = |queue_control: msg::queue::Control,
                             queue_stream: msg::queue::Stream,
                             pending_validation: bool| {
        let payload = initial_payload.take();
        if payload.is_none() {
            tracing::error!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                "create_stream called more than once for FlowInit"
            );
        }
        if let Some(p) = payload {
            if is_fin || !p.is_empty() {
                queue_stream.push(
                    msg::Stream::Data {
                        offset: VarInt::ZERO,
                        fin: is_fin,
                        payload: p,
                    }
                    .into(),
                );
            }
        }

        let local_queue_id = queue_control.queue_id();
        let writer = Writer::new_server(
            frame_tx.clone(),
            peer.path_entry.clone(),
            stream_id,
            queue_control,
        );
        let reader = if pending_validation {
            Reader::new_server_pending(
                frame_tx.clone(),
                peer.path_entry.clone(),
                stream_id,
                queue_stream,
                is_fin,
            )
        } else {
            Reader::new_server(
                frame_tx.clone(),
                peer.path_entry.clone(),
                stream_id,
                queue_stream,
                is_fin,
            )
        };

        (
            local_queue_id,
            PendingValidation::new(Stream::new(reader, writer)),
        )
    };

    match peer.attempt_dedup.check_attempt_id(attempt_id) {
        Ok(()) => {
            let register_result = {
                let _guard = counters.rx_init_register_time.start();
                peer.flows.try_register(stream_id, create_queue)
            };
            match register_result {
                Ok((queue_control, queue_stream)) => {
                    let dispatched = acceptor_registry.with_acceptor(acceptor_id, |acceptor| {
                        let (local_queue_id, stream) = {
                            let _guard = counters.rx_init_create_stream_time.start();
                            create_stream(queue_control, queue_stream, false)
                        };
                        let dispatch_result = {
                            let _guard = counters.rx_init_dispatch_time.start();
                            acceptor.handle_request(stream)
                        };
                        (local_queue_id, dispatch_result)
                    });

                    let Some((local_queue_id, dispatch_result)) = dispatched else {
                        tracing::debug!(
                            attempt_id = attempt_id.as_u64(),
                            stream_id = stream_id.as_u64(),
                            acceptor_id = acceptor_id.as_u64(),
                            "FlowInit rejected - acceptor not found"
                        );
                        push_reset_frame(
                            response_frames,
                            counters,
                            &peer.path_entry,
                            peer_queue_id,
                            stream_id,
                            error::ACCEPTOR_NOT_FOUND,
                        );
                        return;
                    };

                    match dispatch_result {
                        Ok(waker) => {
                            counters.flow_accepted.add(1);
                            let _ = waker_sink.send(waker);
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                server_queue_id = local_queue_id.as_u64(),
                                "FlowInit accepted - dispatched to acceptor"
                            );
                        }
                        Err(mut reject) => {
                            reject.request.disable();
                            counters.rx_init_acceptor_reset.add(1);
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                reset_code = reject.reset_code().as_u64(),
                                "FlowInit rejected - acceptor requested reset"
                            );
                            push_reset_frame(
                                response_frames,
                                counters,
                                &peer.path_entry,
                                peer_queue_id,
                                stream_id,
                                reject.reset_code(),
                            );
                        }
                    }
                }
                Err(local_queue_id) => {
                    tracing::debug!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        local_queue_id = local_queue_id.as_u64(),
                        "FlowInit rejected - stream_id reused by client"
                    );
                    push_reset_frame(
                        response_frames,
                        counters,
                        &peer.path_entry,
                        peer_queue_id,
                        stream_id,
                        error::STREAM_ID_ERROR,
                    );
                }
            }
        }
        Err(AttemptDedupError::Duplicate) => {
            counters.rx_init_dup.add(1);
            tracing::trace!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                "Duplicate FlowInit attempt_id - dropping"
            );
        }
        Err(AttemptDedupError::TooOld) => {
            counters.rx_init_too_old.add(1);

            let register_result = {
                let _guard = counters.rx_init_register_time.start();
                peer.flows.try_register(stream_id, create_queue)
            };
            match register_result {
                Ok((queue_control, queue_stream)) => {
                    let dispatched = acceptor_registry.with_acceptor(acceptor_id, |acceptor| {
                        let (local_queue_id, stream) = {
                            let _guard = counters.rx_init_create_stream_time.start();
                            create_stream(queue_control, queue_stream, true)
                        };
                        let dispatch_result = {
                            let _guard = counters.rx_init_dispatch_time.start();
                            acceptor.handle_pending(stream)
                        };
                        (local_queue_id, dispatch_result)
                    });

                    let Some((local_queue_id, dispatch_result)) = dispatched else {
                        counters.rx_init_no_acceptor.add(1);
                        tracing::debug!(
                            attempt_id = attempt_id.as_u64(),
                            stream_id = stream_id.as_u64(),
                            acceptor_id = acceptor_id.as_u64(),
                            "FlowInit rejected - acceptor not found"
                        );
                        push_reset_frame(
                            response_frames,
                            counters,
                            &peer.path_entry,
                            peer_queue_id,
                            stream_id,
                            error::ACCEPTOR_NOT_FOUND,
                        );
                        return;
                    };

                    match dispatch_result {
                        Ok(acceptor::Dispatch { action, waker }) => {
                            let _ = waker_sink.send(waker);
                            match action {
                                acceptor::PendingAction::Accepted => {
                                    counters.rx_init_accepted.add(1);
                                    counters.flow_accepted.add(1);
                                    let request = flow::Request {
                                        credential_id: credentials.id,
                                        stream_id,
                                    };
                                    let stream_entry = msg::Stream::FlowValidated.into();
                                    if let Ok(waker) = queue_dispatcher.send_stream(
                                        local_queue_id,
                                        Some(peer_queue_id),
                                        &request,
                                        stream_entry,
                                    ) {
                                        let _ = waker_sink.send(waker);
                                    }
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        server_queue_id = local_queue_id.as_u64(),
                                        "FlowInit accepted without retry"
                                    );
                                }
                                acceptor::PendingAction::AcceptedWithRetry => {
                                    counters.rx_init_accepted_retry.add(1);
                                    counters.flow_pending.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        server_queue_id = local_queue_id.as_u64(),
                                        "FlowInit accepted with retry"
                                    );
                                    push_validate_request_frame(
                                        response_frames,
                                        counters,
                                        &peer.path_entry,
                                        source_sender_id,
                                        local_queue_id,
                                        peer_queue_id,
                                        attempt_id,
                                        stream_id,
                                    );
                                }
                            }
                        }
                        Err(mut reject) => {
                            reject.request.disable();
                            counters.rx_init_reject.add(1);
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                reset_code = reject.reset_code().as_u64(),
                                "FlowInit rejected"
                            );
                            push_reset_frame(
                                response_frames,
                                counters,
                                &peer.path_entry,
                                peer_queue_id,
                                stream_id,
                                reject.reset_code(),
                            );
                        }
                    }
                }
                Err(local_queue_id) => {
                    counters.rx_init_retx.add(1);
                    tracing::trace!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowInit retransmission of existing flow - dropping"
                    );
                }
            }
        }
    }
}

fn push_reset_frame(
    response_frames: &mut PriorityInput,
    counters: &counters::Dispatch,
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    dest_queue_id: VarInt,
    stream_id: VarInt,
    error_code: VarInt,
) {
    push_reset_frame_with_target(
        response_frames,
        counters,
        path_secret_entry,
        dest_queue_id,
        stream_id,
        ResetTarget::Both,
        error_code,
    );
}

fn push_reset_frame_with_target(
    response_frames: &mut PriorityInput,
    counters: &counters::Dispatch,
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    dest_queue_id: VarInt,
    stream_id: VarInt,
    reset_target: ResetTarget,
    error_code: VarInt,
) {
    let frame = Frame {
        header: Header::FlowReset {
            dest_queue_id,
            stream_id,
            reset_target,
            error_code,
        },
        source_sender_id: UNSET_SOURCE_SENDER_ID,
        payload: ByteVec::new(),
        path_secret_entry: path_secret_entry.clone(),
        completion: None,
        status: Default::default(),
        ttl: DEFAULT_TTL,
        transmission_time: None,
    };
    counters.on_response_frame(&frame.header);
    response_frames.push(frame.into());
}

fn push_validate_request_frame(
    response_frames: &mut PriorityInput,
    counters: &counters::Dispatch,
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    dest_sender_id: VarInt,
    source_queue_id: VarInt,
    dest_queue_id: VarInt,
    attempt_id: VarInt,
    stream_id: VarInt,
) {
    let frame = Frame {
        header: Header::FlowValidateRequest {
            dest_sender_id,
            queue_pair: QueuePair {
                source_queue_id,
                dest_queue_id,
            },
            attempt_id,
            stream_id,
        },
        source_sender_id: UNSET_SOURCE_SENDER_ID,
        payload: ByteVec::new(),
        path_secret_entry: path_secret_entry.clone(),
        completion: None,
        status: Default::default(),
        ttl: DEFAULT_TTL,
        transmission_time: None,
    };
    counters.on_response_frame(&frame.header);
    response_frames.push(frame.into());
}

// ── FlowValidateRequest ───────────────────────────────────────────────────

fn handle_flow_validate_request(
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    credentials: &Credentials,
    _dest_sender_id: VarInt,
    queue_pair: QueuePair,
    attempt_id: VarInt,
    stream_id: VarInt,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    response_frames: &mut PriorityInput,
) {
    let local_queue_id = queue_pair.dest_queue_id;

    let request = flow::Request {
        credential_id: credentials.id,
        stream_id,
    };

    match queue_dispatcher.validate_stream(local_queue_id, &request) {
        Ok(()) => {
            counters.rx_validate_ok.add(1);
            tracing::debug!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                "FlowValidateRequest validated"
            );
            let frame = Frame {
                header: Header::FlowInitValidate {
                    queue_pair: queue_pair.reverse(),
                    attempt_id,
                    stream_id,
                },
                source_sender_id: UNSET_SOURCE_SENDER_ID,
                payload: ByteVec::new(),
                path_secret_entry: path_secret_entry.clone(),
                completion: None,
                status: Default::default(),
                ttl: DEFAULT_TTL,
                transmission_time: None,
            };
            counters.on_response_frame(&frame.header);
            response_frames.push(frame.into());
        }
        Err(_) => {
            counters.rx_validate_failed.add(1);
            tracing::warn!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                "FlowValidateRequest validation failed"
            );
            let frame = Frame {
                header: Header::FlowReset {
                    dest_queue_id: queue_pair.source_queue_id,
                    stream_id,
                    reset_target: ResetTarget::Both,
                    error_code: error::FLOW_VALIDATION_FAILED,
                },
                source_sender_id: UNSET_SOURCE_SENDER_ID,
                payload: ByteVec::new(),
                path_secret_entry: path_secret_entry.clone(),
                completion: None,
                status: Default::default(),
                ttl: DEFAULT_TTL,
                transmission_time: None,
            };
            counters.on_response_frame(&frame.header);
            response_frames.push(frame.into());
        }
    }
}

// ── FlowInitValidate ──────────────────────────────────────────────────────

fn handle_flow_init_validate(
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    credentials: &Credentials,
    queue_pair: QueuePair,
    attempt_id: VarInt,
    stream_id: VarInt,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    response_frames: &mut PriorityInput,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = queue_pair.dest_queue_id;

    let request = flow::Request {
        credential_id: credentials.id,
        stream_id,
    };

    match queue_dispatcher.validate_stream(local_queue_id, &request) {
        Ok(()) => {
            counters.rx_init_validate_ok.add(1);

            let stream_entry = msg::Stream::FlowValidated.into();
            match queue_dispatcher.send_stream(
                local_queue_id,
                Some(queue_pair.source_queue_id),
                &request,
                stream_entry,
            ) {
                Ok(waker) => {
                    let _ = waker_sink.send(waker);
                    tracing::debug!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        "FlowInitValidate validated"
                    );
                }
                Err(_) => {
                    counters.rx_init_validate_dispatch_failed.add(1);
                    tracing::warn!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowInitValidate failed to send FlowValidated - sending reset"
                    );
                    push_reset_frame(
                        response_frames,
                        counters,
                        path_secret_entry,
                        queue_pair.source_queue_id,
                        stream_id,
                        error::FLOW_VALIDATION_FAILED,
                    );
                }
            }
        }
        Err(_) => {
            counters.rx_init_validate_validation_failed.add(1);
            tracing::warn!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowInitValidate validation failed - sending reset"
            );
            push_reset_frame(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                error::FLOW_VALIDATION_FAILED,
            );
        }
    }
}

// ── FlowData ──────────────────────────────────────────────────────────────

fn handle_flow_data(
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    credentials: &Credentials,
    queue_pair: QueuePair,
    stream_id: VarInt,
    offset: VarInt,
    is_fin: bool,
    buf: BytesMut,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    response_frames: &mut PriorityInput,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = queue_pair.dest_queue_id;

    let request = flow::Request {
        credential_id: credentials.id,
        stream_id,
    };

    let payload_len = buf.len();
    let entry = msg::Stream::Data {
        offset,
        fin: is_fin,
        payload: buf,
    }
    .into();

    match queue_dispatcher.send_stream(
        local_queue_id,
        Some(queue_pair.source_queue_id),
        &request,
        entry,
    ) {
        Ok(waker) => {
            let _ = waker_sink.send(waker);
            counters.rx_data_ok.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                offset = offset.as_u64(),
                payload_len,
                is_fin,
                "FlowData dispatched"
            );
        }
        Err(flow::queue::Error::Unallocated(_)) => {
            counters.rx_data_unallocated.add(1);
            tracing::warn!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for unallocated queue - sending reset"
            );
            push_reset_frame(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                error::QUEUE_UNALLOCATED,
            );
        }
        Err(flow::queue::Error::HalfClosed(_)) => {
            counters.rx_data_half_closed.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for half-closed stream - dropping"
            );
        }
        Err(flow::queue::Error::ValidationFailed(_, reason)) => {
            counters.on_data_validation_failed(reason);
            if let Some(error_code) = reason.as_reset_code() {
                tracing::debug!(
                    stream_id = stream_id.as_u64(),
                    queue_id = local_queue_id.as_u64(),
                    ?reason,
                    "FlowData validation failed - sending reset"
                );
                push_reset_frame_with_target(
                    response_frames,
                    counters,
                    path_secret_entry,
                    queue_pair.source_queue_id,
                    stream_id,
                    ResetTarget::Both,
                    error_code,
                );
            } else {
                tracing::trace!(
                    stream_id = stream_id.as_u64(),
                    queue_id = local_queue_id.as_u64(),
                    "FlowData for previous occupant - dropping"
                );
            }
        }
        Err(flow::queue::Error::PermanentlyClosed) => {
            counters.rx_data_perm_closed.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for permanently closed queue"
            );
        }
    }
}

// ── FlowControl ───────────────────────────────────────────────────────────

fn handle_flow_control(
    path_secret_entry: &std::sync::Arc<PathSecretEntry>,
    credentials: &Credentials,
    queue_pair: QueuePair,
    stream_id: VarInt,
    buf: BytesMut,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    response_frames: &mut PriorityInput,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = queue_pair.dest_queue_id;

    let request = flow::Request {
        credential_id: credentials.id,
        stream_id,
    };

    let payload_len = buf.len();
    let entry = msg::Control::Frames { payload: buf }.into();

    match queue_dispatcher.send_control(
        local_queue_id,
        Some(queue_pair.source_queue_id),
        &request,
        entry,
    ) {
        Ok(waker) => {
            let _ = waker_sink.send(waker);
            counters.rx_flow_control_ok.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                payload_len,
                "FlowControl dispatched"
            );
        }
        Err(flow::queue::Error::Unallocated(_)) => {
            counters.rx_flow_control_unallocated.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for unallocated queue - sending reset"
            );
            push_reset_frame_with_target(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                ResetTarget::Both,
                error::QUEUE_UNALLOCATED,
            );
        }
        Err(flow::queue::Error::HalfClosed(_)) => {
            counters.rx_flow_control_half_closed.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for half-closed control queue - dropping"
            );
        }
        Err(flow::queue::Error::ValidationFailed(_, reason)) => {
            counters.on_flow_control_validation_failed(reason);
            if let Some(error_code) = reason.as_reset_code() {
                tracing::debug!(
                    stream_id = stream_id.as_u64(),
                    queue_id = local_queue_id.as_u64(),
                    ?reason,
                    "FlowControl validation failed - sending reset"
                );
                push_reset_frame_with_target(
                    response_frames,
                    counters,
                    path_secret_entry,
                    queue_pair.source_queue_id,
                    stream_id,
                    ResetTarget::Both,
                    error_code,
                );
            } else {
                tracing::trace!(
                    stream_id = stream_id.as_u64(),
                    queue_id = local_queue_id.as_u64(),
                    "FlowControl for previous occupant - dropping"
                );
            }
        }
        Err(flow::queue::Error::PermanentlyClosed) => {
            counters.rx_flow_control_perm_closed.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for permanently closed queue"
            );
        }
    }
}

// ── FlowReset ─────────────────────────────────────────────────────────────

fn handle_flow_reset(
    credentials: &Credentials,
    dest_queue_id: VarInt,
    stream_id: VarInt,
    reset_target: ResetTarget,
    error_code: VarInt,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    waker_sink: &mut impl channel::UnboundedSender<AutoWake>,
) {
    let local_queue_id = dest_queue_id;

    let request = flow::Request {
        credential_id: credentials.id,
        stream_id,
    };

    match reset_target {
        ResetTarget::Both => {
            counters.rx_reset_both.add(1);
            let stream_entry = msg::Stream::Reset { error_code }.into();
            let control_entry = msg::Control::Reset { error_code }.into();
            let (waker_a, waker_b) = queue_dispatcher.send_both(
                local_queue_id,
                None,
                &request,
                stream_entry,
                control_entry,
            );
            let _ = waker_sink.send(waker_a);
            let _ = waker_sink.send(waker_b);

            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "FlowReset(Both) dispatched"
            );
        }
        ResetTarget::Stream => {
            counters.rx_reset_stream.add(1);
            let stream_entry = msg::Stream::Reset { error_code }.into();
            if let Ok(waker) =
                queue_dispatcher.send_stream(local_queue_id, None, &request, stream_entry)
            {
                let _ = waker_sink.send(waker);
            }

            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "FlowReset(Stream) dispatched"
            );
        }
        ResetTarget::Control => {
            counters.rx_reset_control.add(1);
            let control_entry = msg::Control::Reset { error_code }.into();
            if let Ok(waker) =
                queue_dispatcher.send_control(local_queue_id, None, &request, control_entry)
            {
                let _ = waker_sink.send(waker);
            }

            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "FlowReset(Control) dispatched"
            );
        }
    }
}

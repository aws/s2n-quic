// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Inbound packet processing: decrypt, deduplicate, and dispatch frames to flow queues.
//!
//! A single received packet may contain multiple frames (the frame aggregation model).
//! After decryption and packet-number deduplication, we iterate the frame metadata region
//! and dispatch each frame to its appropriate handler based on the frame header type.

use crate::{
    byte_vec::ByteVec,
    acceptor,
    credentials::Credentials,
    flow,
    intrusive_queue::{Entry, Queue},
    packet::{
        self,
        datagram::{QueuePair, ResetTarget, RoutingInfo},
    },
    path::secret::Map as PathSecretMap,
    path::secret::map::Entry as PathSecretEntry,
    socket::{channel, pool::descriptor},
    stream3::{
        endpoint::{
            counters, decode, msg,
            recv::{self, AckState, AttemptDedupError},
            reset_error,
        },
        frame::{Frame, Header, SubmissionSender, DEFAULT_TTL},
        Reader, Stream, Writer,
    },
};
use bytes::BytesMut;
use s2n_quic_core::varint::VarInt;

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
    UnsupportedRoutingInfo {
        routing_info: RoutingInfo,
    },
}

/// Process a received datagram packet.
///
/// Authenticates (decrypt), deduplicates by packet number, updates ACK state, then
/// dispatches each frame in the packet to its type-specific handler. Response frames
/// (ACKs, FlowValidateRequest, FlowReset) are emitted to `response_tx`.
pub(crate) fn process<Clk>(
    packet: Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
    recv_cache: &mut recv::Cache,
    path_secret_map: &PathSecretMap,
    acceptor_registry: &acceptor::Registry<Stream>,
    frame_tx: &SubmissionSender,
    response_tx: &mut impl channel::UnboundedSender<Queue<Frame>>,
    sender_tx: &mut impl channel::UnboundedSender<msg::Sender>,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    clock: &Clk,
    counters: &counters::Dispatch,
) -> Result<(), Error>
where
    Clk: s2n_quic_core::time::Clock + ?Sized,
{
    let credentials = *packet.credentials();
    let packet_number = packet.packet_number();
    let routing_info = packet.routing_info();
    let idle_timeout = recv_cache.idle_timeout;

    let source_sender_id = match routing_info {
        RoutingInfo::SenderId { source_sender_id } => source_sender_id,
        RoutingInfo::None => return Err(Error::MissingSenderId),
        _ => return Err(Error::UnsupportedRoutingInfo { routing_info }),
    };

    // Get or create peer receive state
    let mut control_out = Vec::new();
    let Some(peer) = recv_cache.get_or_insert(
        &credentials,
        source_sender_id,
        path_secret_map,
        clock,
        &mut control_out,
    ) else {
        return Err(Error::PeerStateLookup {
            credentials,
            control_out,
        });
    };

    // Collect information about the packet layout before decryption.
    let app_header_slice: &[u8] = packet.application_header();
    let decrypt_len = packet.decrypt_into_len();
    let ecn = packet.storage().ecn();

    // Decrypt payload bytes into a BytesMut buffer.
    let mut decrypted = BytesMut::with_capacity(decrypt_len);
    let written = packet
        .decrypt_into(&peer.opener, bytes::BufMut::chunk_mut(&mut decrypted))
        .map_err(|_| Error::Decryption {
            credentials,
            packet_number,
        })?;
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
    if peer
        .ack_space
        .filter
        .on_packet_number(packet_number)
        .is_err()
    {
        return Err(Error::Duplicate {
            credentials,
            packet_number,
        });
    }

    // Update activity and ACK tracking
    peer.update_activity(clock, idle_timeout);
    peer.ecn_counts.increment(ecn);
    peer.ack_space
        .on_packet_received(packet_number, clock.get_time());
    peer.ack_state = AckState::Scheduled;

    let mut payload_storage = decrypted;

    let mut response_frames = Queue::new();

    // Multi-frame packet: `app_header_slice` contains the per-frame metadata
    // (Header type tag + optional payload_len VarInt) and `payload_storage`
    // contains the concatenated, decrypted frame payloads.

    for result in decode::decode_frames(app_header_slice) {
        match result {
            Ok((header, frame_payload_len)) => {
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
                // Split the frame's payload out of the shared storage.
                let frame_payload = payload_storage.split_to(frame_payload_len);
                dispatch_decoded_frame(
                    header,
                    source_sender_id,
                    frame_payload,
                    peer,
                    &credentials,
                    acceptor_registry,
                    frame_tx,
                    queue_dispatcher,
                    sender_tx,
                    counters,
                    &mut response_frames,
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
    acceptor_registry: &acceptor::Registry<Stream>,
    frame_tx: &SubmissionSender,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    sender_tx: &mut impl channel::UnboundedSender<msg::Sender>,
    counters: &counters::Dispatch,
    response_frames: &mut Queue<Frame>,
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
            );
        }
        Header::FlowControl { queue_pair, stream_id } => {
            handle_flow_control(
                &peer.path_entry,
                credentials,
                queue_pair,
                stream_id,
                payload,
                queue_dispatcher,
                counters,
                response_frames,
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
            );
        }
        Header::Control { dest_sender_id } => {
            let message = msg::Sender::Ack {
                local_sender_id: dest_sender_id,
                payload,
            };
            if sender_tx.send(message).is_err() {
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
    acceptor_registry: &acceptor::Registry<Stream>,
    frame_tx: &SubmissionSender,
    queue_dispatcher: &mut msg::queue::Dispatcher,
    counters: &counters::Dispatch,
    response_frames: &mut Queue<Frame>,
) {
    let create_queue = |handle| {
        let (queue_control, queue_stream) = queue_dispatcher.alloc_or_grow(handle, Some(peer_queue_id));
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
        let writer = Writer::new_server(frame_tx.clone(), peer.path_entry.clone(), stream_id, queue_control);
        let reader = if pending_validation {
            Reader::new_server_pending(frame_tx.clone(), peer.path_entry.clone(), stream_id, queue_stream)
        } else {
            Reader::new_server(frame_tx.clone(), peer.path_entry.clone(), stream_id, queue_stream)
        };

        (local_queue_id, Stream::new(reader, writer))
    };

    match peer.attempt_dedup.check_attempt_id(attempt_id) {
        Ok(()) => match peer.flows.try_register(stream_id, create_queue) {
            Ok((queue_control, queue_stream)) => {
                let (local_queue_id, stream) = create_stream(queue_control, queue_stream, false);

                match acceptor_registry.dispatch(acceptor_id, stream) {
                    Ok(()) => {
                        counters.flow_accepted.add(1);
                        tracing::debug!(
                            attempt_id = attempt_id.as_u64(),
                            stream_id = stream_id.as_u64(),
                            acceptor_id = acceptor_id.as_u64(),
                            server_queue_id = local_queue_id.as_u64(),
                            "FlowInit accepted - dispatched to acceptor"
                        );
                    }
                    Err(acceptor::DispatchError::AcceptorNotFound) => {
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
                            reset_error::ACCEPTOR_NOT_FOUND,
                        );
                    }
                    Err(acceptor::DispatchError::Reset { reset_code }) => {
                        tracing::debug!(
                            attempt_id = attempt_id.as_u64(),
                            stream_id = stream_id.as_u64(),
                            acceptor_id = acceptor_id.as_u64(),
                            reset_code = reset_code.as_u64(),
                            "FlowInit rejected - acceptor requested reset"
                        );
                        push_reset_frame(
                            response_frames,
                            counters,
                            &peer.path_entry,
                            peer_queue_id,
                            stream_id,
                            reset_code,
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
                    reset_error::STREAM_ID_ERROR,
                );
            }
        },
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

            match peer.flows.try_register(stream_id, create_queue) {
                Ok((queue_control, queue_stream)) => {
                    let (local_queue_id, stream) = create_stream(queue_control, queue_stream, true);
                    match acceptor_registry.dispatch_pending(acceptor_id, stream) {
                        Ok(acceptor::PendingAction::Accepted) => {
                            counters.rx_init_accepted.add(1);
                            counters.flow_accepted.add(1);
                            let request = flow::Request {
                                credential_id: credentials.id,
                                stream_id,
                            };
                            let stream_entry = msg::Stream::FlowValidated.into();
                            let _ = queue_dispatcher.send_stream(
                                local_queue_id,
                                Some(peer_queue_id),
                                &request,
                                stream_entry,
                            );
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                server_queue_id = local_queue_id.as_u64(),
                                "FlowInit accepted without retry - acceptor doesn't require dedup"
                            );
                        }
                        Ok(acceptor::PendingAction::AcceptedWithRetry) => {
                            counters.rx_init_accepted_retry.add(1);
                            counters.flow_pending.add(1);
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                server_queue_id = local_queue_id.as_u64(),
                                "FlowInit accepted with retry - requesting validation from client"
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
                        Ok(acceptor::PendingAction::Reject { reset_code }) => {
                            counters.rx_init_reject.add(1);
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                reset_code = reset_code.as_u64(),
                                "FlowInit rejected - acceptor rejected pending request"
                            );
                            push_reset_frame(
                                response_frames,
                                counters,
                                &peer.path_entry,
                                peer_queue_id,
                                stream_id,
                                reset_code,
                            );
                        }
                        Err(acceptor::DispatchError::AcceptorNotFound) => {
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
                                reset_error::ACCEPTOR_NOT_FOUND,
                            );
                        }
                        Err(acceptor::DispatchError::Reset { reset_code }) => {
                            counters.rx_init_acceptor_reset.add(1);
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                acceptor_id = acceptor_id.as_u64(),
                                reset_code = reset_code.as_u64(),
                                "FlowInit rejected - acceptor requested reset"
                            );
                            push_reset_frame(
                                response_frames,
                                counters,
                                &peer.path_entry,
                                peer_queue_id,
                                stream_id,
                                reset_code,
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
    response_frames: &mut Queue<Frame>,
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
    response_frames: &mut Queue<Frame>,
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
    counters.on_sent_frame(&frame.header);
    response_frames.push_back(frame.into());
}

fn push_validate_request_frame(
    response_frames: &mut Queue<Frame>,
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
    counters.on_sent_frame(&frame.header);
    response_frames.push_back(frame.into());
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
    response_frames: &mut Queue<Frame>,
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
            counters.on_sent_frame(&frame.header);
            response_frames.push_back(frame.into());
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
                    error_code: reset_error::STALE_STATE,
                },
                source_sender_id: UNSET_SOURCE_SENDER_ID,
                payload: ByteVec::new(),
                path_secret_entry: path_secret_entry.clone(),
                completion: None,
                status: Default::default(),
                ttl: DEFAULT_TTL,
                transmission_time: None,
            };
            counters.on_sent_frame(&frame.header);
            response_frames.push_back(frame.into());
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
    response_frames: &mut Queue<Frame>,
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
                Ok(()) => {
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
                        reset_error::STALE_STATE,
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
                reset_error::STALE_STATE,
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
    response_frames: &mut Queue<Frame>,
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
        Ok(()) => {
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
                reset_error::STALE_STATE,
            );
        }
        Err(flow::queue::Error::HalfClosed(_)) => {
            counters.rx_data_half_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for half-closed stream - sending reset"
            );
            push_reset_frame_with_target(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                ResetTarget::Stream,
                reset_error::STALE_STATE,
            );
        }
        Err(flow::queue::Error::FullyClosed(_)) => {
            counters.rx_data_fully_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for fully closed queue - sending reset"
            );
            push_reset_frame_with_target(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                ResetTarget::Both,
                reset_error::STALE_STATE,
            );
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
    response_frames: &mut Queue<Frame>,
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
        Ok(()) => {
            counters.rx_control_ok.add(1);
            tracing::trace!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                payload_len,
                "FlowControl dispatched"
            );
        }
        Err(flow::queue::Error::Unallocated(_)) => {
            counters.rx_control_unallocated.add(1);
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
                reset_error::STALE_STATE,
            );
        }
        Err(flow::queue::Error::HalfClosed(_)) => {
            counters.rx_control_half_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for half-closed control queue - sending reset"
            );
            push_reset_frame_with_target(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                ResetTarget::Control,
                reset_error::STALE_STATE,
            );
        }
        Err(flow::queue::Error::FullyClosed(_)) => {
            counters.rx_control_fully_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for fully closed queue - sending reset"
            );
            push_reset_frame_with_target(
                response_frames,
                counters,
                path_secret_entry,
                queue_pair.source_queue_id,
                stream_id,
                ResetTarget::Both,
                reset_error::STALE_STATE,
            );
        }
        Err(flow::queue::Error::PermanentlyClosed) => {
            counters.rx_control_perm_closed.add(1);
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
            queue_dispatcher.send_both(local_queue_id, None, &request, stream_entry, control_entry);

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
            let _ = queue_dispatcher.send_stream(local_queue_id, None, &request, stream_entry);

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
            let _ = queue_dispatcher.send_control(local_queue_id, None, &request, control_entry);

            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                error_code = error_code.as_u64(),
                "FlowReset(Control) dispatched"
            );
        }
    }
}

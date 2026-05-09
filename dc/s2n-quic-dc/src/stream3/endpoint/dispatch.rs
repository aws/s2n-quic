// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Inbound packet processing: decrypt, deduplicate, and dispatch frames to flow queues.
//!
//! A single received packet may contain multiple frames (the frame aggregation model).
//! After decryption and packet-number deduplication, we iterate the frame metadata region
//! and dispatch each frame to its appropriate handler based on the frame header type.

use crate::{
    acceptor,
    credentials::Credentials,
    flow,
    intrusive_queue::{Entry, Queue},
    packet::{
        self,
        datagram::{QueuePair, ResetTarget, RoutingInfo},
    },
    path::secret::Map as PathSecretMap,
    socket::{channel, pool::descriptor},
    stream3::{
        endpoint::{
            counters, msg,
            recv::{self, AckState, AttemptDedupError},
            reset_error,
        },
        frame::{Frame, SubmissionSender},
        Stream,
    },
};
use bytes::BytesMut;
use s2n_quic_core::varint::VarInt;

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
pub(crate) fn process<Clk>(
    packet: Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
    recv_cache: &mut recv::Cache,
    path_secret_map: &PathSecretMap,
    acceptor_registry: &acceptor::Registry<Stream>,
    frame_tx: &SubmissionSender,
    response_tx: &mut impl channel::UnboundedSender<Queue<Frame>>,
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

    let Some(source_sender_id) = routing_info.source_sender_id() else {
        return Err(Error::MissingSenderId);
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

    // Decrypt
    let len = packet.decrypt_into_len();
    let mut buf = BytesMut::with_capacity(len);

    let written = packet
        .decrypt_into(&peer.opener, bytes::BufMut::chunk_mut(&mut buf))
        .map_err(|_| Error::Decryption {
            credentials,
            packet_number,
        })?;

    unsafe {
        debug_assert_eq!(written, len);
        buf.set_len(len);
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
    let ecn = packet.storage().ecn();
    peer.ecn_counts.increment(ecn);
    peer.ack_space
        .on_packet_received(packet_number, clock.get_time());
    peer.ack_state = AckState::Scheduled;

    // Dispatch based on routing info
    //
    // TODO: When the multi-frame packet format lands, this becomes an iteration over
    // decoded frame headers in the metadata region. For now, one routing_info per packet.
    let routing_info = packet.routing_info();
    let mut response_frames = Queue::new();

    match routing_info {
        RoutingInfo::None => {
            counters.rx_none.add(1);
        }
        RoutingInfo::FlowInit {
            source_sender_id: _,
            source_queue_id: peer_queue_id,
            dest_acceptor_id: acceptor_id,
            attempt_id,
            stream_id,
            is_fin,
        } => {
            counters.rx_init.add(1);
            handle_flow_init(
                peer,
                &credentials,
                peer_queue_id,
                acceptor_id,
                attempt_id,
                stream_id,
                is_fin,
                buf,
                acceptor_registry,
                frame_tx,
                queue_dispatcher,
                counters,
                &mut response_frames,
            );
        }
        RoutingInfo::FlowValidateRequest {
            source_sender_id: _,
            dest_sender_id,
            queue_pair,
            attempt_id,
            stream_id,
        } => {
            counters.rx_validate.add(1);
            handle_flow_validate_request(
                &credentials,
                dest_sender_id,
                queue_pair,
                attempt_id,
                stream_id,
                queue_dispatcher,
                counters,
                &mut response_frames,
            );
        }
        RoutingInfo::FlowInitValidate {
            source_sender_id: _,
            queue_pair,
            attempt_id,
            stream_id,
        } => {
            counters.rx_init_validate.add(1);
            handle_flow_init_validate(
                &credentials,
                queue_pair,
                attempt_id,
                stream_id,
                queue_dispatcher,
                counters,
                &mut response_frames,
            );
        }
        RoutingInfo::FlowData {
            source_sender_id: _,
            queue_pair,
            stream_id,
            offset,
            is_fin,
        } => {
            counters.rx_data.add(1);
            handle_flow_data(
                &credentials,
                queue_pair,
                stream_id,
                offset,
                is_fin,
                buf,
                queue_dispatcher,
                counters,
                &mut response_frames,
            );
        }
        RoutingInfo::FlowControl {
            source_sender_id: _,
            queue_pair,
            stream_id,
        } => {
            counters.rx_control.add(1);
            handle_flow_control(
                &credentials,
                queue_pair,
                stream_id,
                buf,
                queue_dispatcher,
                counters,
                &mut response_frames,
            );
        }
        RoutingInfo::FlowReset {
            source_sender_id: _,
            dest_queue_id,
            stream_id,
            reset_target,
            error_code,
        } => {
            counters.rx_reset.add(1);
            handle_flow_reset(
                &credentials,
                dest_queue_id,
                stream_id,
                reset_target,
                error_code,
                queue_dispatcher,
                counters,
            );
        }
        RoutingInfo::SenderId { .. } => {
            // TODO: multi-frame packet dispatch — iterate frame headers in the
            // application header region and dispatch each frame individually.
        }
    }

    let _ = response_tx.send(response_frames);
    Ok(())
}

// ── FlowInit ──────────────────────────────────────────────────────────────

fn handle_flow_init(
    peer: &mut recv::Context,
    credentials: &Credentials,
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
    // TODO: port FlowInit handling (attempt dedup, queue allocation, acceptor dispatch)
    // This is the largest handler — will be filled in next pass.
}

// ── FlowValidateRequest ───────────────────────────────────────────────────

fn handle_flow_validate_request(
    credentials: &Credentials,
    dest_sender_id: VarInt,
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
            // TODO: emit FlowInitValidate response frame
        }
        Err(_) => {
            counters.rx_validate_failed.add(1);
            tracing::warn!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                "FlowValidateRequest validation failed"
            );
            // TODO: emit FlowReset response frame
        }
    }
}

// ── FlowInitValidate ──────────────────────────────────────────────────────

fn handle_flow_init_validate(
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
                        "FlowInitValidate failed to send FlowValidated"
                    );
                    // TODO: emit FlowReset response frame
                }
            }
        }
        Err(_) => {
            counters.rx_init_validate_validation_failed.add(1);
            tracing::warn!(
                attempt_id = attempt_id.as_u64(),
                stream_id = stream_id.as_u64(),
                "FlowInitValidate validation failed"
            );
            // TODO: emit FlowReset response frame
        }
    }
}

// ── FlowData ──────────────────────────────────────────────────────────────

fn handle_flow_data(
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
                "FlowData for unallocated queue"
            );
            // TODO: emit FlowReset response frame
        }
        Err(flow::queue::Error::HalfClosed(_)) => {
            counters.rx_data_half_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for half-closed stream"
            );
            // TODO: emit FlowReset(Stream) response frame
        }
        Err(flow::queue::Error::FullyClosed(_)) => {
            counters.rx_data_fully_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowData for fully closed queue"
            );
            // TODO: emit FlowReset(Both) response frame
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
                "FlowControl for unallocated queue"
            );
            // TODO: emit FlowReset(Both) response frame
        }
        Err(flow::queue::Error::HalfClosed(_)) => {
            counters.rx_control_half_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for half-closed control"
            );
            // TODO: emit FlowReset(Control) response frame
        }
        Err(flow::queue::Error::FullyClosed(_)) => {
            counters.rx_control_fully_closed.add(1);
            tracing::debug!(
                stream_id = stream_id.as_u64(),
                queue_id = local_queue_id.as_u64(),
                "FlowControl for fully closed queue"
            );
            // TODO: emit FlowReset(Both) response frame
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

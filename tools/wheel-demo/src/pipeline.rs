// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Common pipeline components for send and receive paths.
//!
//! Both the client and server need symmetric send/receive capabilities:
//! - Client: sends data datagrams, receives ACK control packets
//! - Server: receives data datagrams, sends ACK control packets
//!
//! This module provides shared pipeline building blocks.

use core::time::Duration;
use s2n_codec::{Encoder as _, EncoderBuffer};
use s2n_quic_core::{
    frame,
    packet::number::{PacketNumberRange, PacketNumberSpace},
    varint::VarInt,
};
use s2n_quic_core::frame::ack::EcnCounts;
use s2n_quic_dc::{
    busy_poll::clock::Timer as BusyPollClock,
    clock::{precision::Clock as _, tokio::Clock as TokioClock},
    congestion,
    credentials::{self, Credentials},
    datagram::batch::Batch,
    intrusive_queue::{Entry, Queue},
    packet::{self, datagram::partial::PartialDatagram, RoutingInfo},
    path::{self, secret::map::Entry as PathSecretEntry},
    random,
    socket::{
        self,
        channel::{
            self, intrusive_queue, FlattenQueue, FlattenSegments, InspectErr, Map, Paced,
            ReceiverExt, Reporter, RouterAdapter, SocketReceiver,
        },
        pool::{self, descriptor},
        rate::Rate,
        recv::router::Router,
        send::wheel::Wheel,
    },
    stream::socket::{BusyPoll, Gso, Options, ReusePort},
};
use s2n_quic_platform::features;
use std::{
    cell::RefCell,
    collections::{hash_map, HashMap},
    io,
    net::SocketAddr,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tracing::info;

// ── Instrumentation ────────────────────────────────────────────────────────

/// Shared counter registry for tracking pipeline metrics
#[derive(Clone, Default)]
pub struct CounterRegistry {
    counters: Arc<Mutex<HashMap<&'static str, Arc<AtomicU64>>>>,
}

impl CounterRegistry {
    pub fn new() -> Self {
        Self {
            counters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a counter with the given label, returning a handle to increment it
    pub fn register(&self, label: &'static str) -> Arc<AtomicU64> {
        let mut counters = self.counters.lock().unwrap();
        counters
            .entry(label)
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone()
    }

    /// Spawn a task that periodically logs all counters in a single line
    pub fn spawn_reporter(&self, interval: Duration) {
        let counters = self.counters.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let counters = counters.lock().unwrap();
                if counters.is_empty() {
                    continue;
                }

                // Collect and reset all counters
                let mut stats: Vec<(&'static str, u64)> = counters
                    .iter()
                    .map(|(label, counter)| (*label, counter.swap(0, Ordering::Relaxed)))
                    .collect();

                // Sort by label for consistent output
                stats.sort_by_key(|(label, _)| *label);

                // Filter non-zero and format as a single line
                let non_zero: Vec<String> = stats
                    .into_iter()
                    .filter(|(_, count)| *count > 0)
                    .map(|(label, count)| format!("{}={}", label, count))
                    .collect();

                if !non_zero.is_empty() {
                    tracing::info!("{}", non_zero.join(" "));
                }
            }
        });
    }
}

// ── Testing Helpers ────────────────────────────────────────────────────────

/// Fast consistent hash for routing packets by credentials
///
/// Combines credentials.id (good entropy) with key_id using a mixing function
#[inline]
fn hash_credentials(credentials: &Credentials) -> u64 {
    // Start with credentials.id which has good entropy
    let mut hash = credentials.id.to_hash();

    // Mix in key_id using a simple but effective mixing function
    // This ensures different paths from the same peer distribute across workers
    let key_id = credentials.key_id.as_u64();
    hash ^= key_id.wrapping_mul(0x9e3779b97f4a7c15); // Golden ratio
    hash = hash.rotate_left(32) ^ key_id;
    hash = hash.wrapping_mul(0x517cc1b727220a95);

    hash
}

/// Create a deterministic test path secret map for demo purposes.
///
/// Uses a fixed secret so client and server (in separate processes) can communicate.
/// The endpoint_type determines whether this is the client or server side.
pub fn create_test_map(
    peer_addr: SocketAddr,
    endpoint_type: s2n_quic_core::endpoint::Type,
) -> s2n_quic_dc::path::secret::map::Map {
    use s2n_quic_dc::{event, path::secret};

    let subscriber = event::tracing::Subscriber::default();
    let signer = secret::stateless_reset::Signer::random();

    let map = secret::Map::new(
        signer,
        100, // capacity
        false,
        s2n_quic_core::time::StdClock::default(),
        subscriber,
    );

    // Insert a deterministic test entry so client and server can communicate
    map.test_insert_deterministic(peer_addr, endpoint_type);

    map
}

// ── Control Packet Processing ──────────────────────────────────────────────

/// Process control frames in a control packet and update send state
fn process_control_frames<Clk, Rand>(
    worker_id: usize,
    packet: &mut Entry<packet::control::decoder::Packet<descriptor::Filled>>,
    context: &mut socket::channel::PathContext<s2n_quic_dc::crypto::awslc::seal::Application>,
    acked: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    lost: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    clock: &Clk,
    random: &mut Rand,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: random::Generator,
{
    let now = clock.get_time();

    // Track ACK processing state
    let mut max_acked_pn = None;
    let mut max_acked_tx_time = None;
    let mut bytes_acked = 0;
    let mut cca_args = None;
    let mut ack_delay = Duration::MAX;

    // Process all ACK frames in the control packet
    for frame in packet.control_frames_mut() {
        let Ok(frame) = frame else {
            tracing::warn!(worker_id, "Failed to decode control frame");
            continue;
        };

        match frame {
            frame::Frame::Ack(ack) => {
                // Process ACK ranges - remove ACKed packets and track metadata
                ack_delay = ack_delay.min(ack.ack_delay());
                process_ack_ranges(
                    &ack,
                    &mut context.packet_number_map,
                    &mut max_acked_pn,
                    &mut max_acked_tx_time,
                    &mut bytes_acked,
                    &mut cca_args,
                    acked,
                );
            }
            frame => {
                tracing::warn!(worker_id, ?frame, "Unexpected control frame type")
            }
        }
    }

    // Update RTT estimator with ACK information
    if let Some((time_sent, cc_info)) = cca_args {
        let rtt_sample = now
            .saturating_duration_since(time_sent)
            .saturating_sub(ack_delay)
            .max(Duration::from_micros(1));

        context.rtt_estimator.update_rtt(
            Duration::ZERO,
            rtt_sample,
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        context.cca.on_packet_ack(
            cc_info.first_sent_time,
            bytes_acked,
            cc_info,
            &context.rtt_estimator,
            random,
            now,
        );
    }

    // Perform loss detection if we ACKed any packets
    if let Some(max_acked_pn) = max_acked_pn {
        if let Some(max_tx_time) = max_acked_tx_time {
            tracing::trace!(
                worker_id,
                max_acked = max_acked_pn.as_u64(),
                bytes_acked,
                ack_delay_us = ack_delay.as_micros(),
                "Processing ACK frame"
            );
            detect_and_retransmit_lost_packets(
                context,
                max_acked_pn,
                max_tx_time,
                lost,
                now,
                random,
            );
        }
    }
}

/// Process ACK ranges and remove ACKed packets from the packet number map
fn process_ack_ranges(
    ack: &frame::Ack<impl frame::ack::AckRanges>,
    packet_number_map: &mut s2n_quic_core::packet::number::Map<Entry<PartialDatagram>>,
    max_acked_pn: &mut Option<VarInt>,
    max_acked_tx_time: &mut Option<s2n_quic_core::time::Timestamp>,
    bytes_acked: &mut usize,
    cca_args: &mut Option<(s2n_quic_core::time::Timestamp, congestion::PacketInfo)>,
    acked: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
) {
    // Process each ACK range
    let mut queue = Queue::new();
    for range in ack.ack_ranges() {
        let pmin = PacketNumberSpace::Initial.new_packet_number(*range.start());
        let pmax = PacketNumberSpace::Initial.new_packet_number(*range.end());
        let range = PacketNumberRange::new(pmin, pmax);

        // Remove ACKed packets from the packet number map
        let mut queue_range = Queue::new();
        for (num, mut entry) in packet_number_map.remove_range(range) {
            let num_varint = unsafe { VarInt::new_unchecked(num.as_u64()) };
            *max_acked_pn = (*max_acked_pn).max(Some(num_varint));

            // Extract transmission metadata
            if let Some(tx_info) = entry.transmission_info.take() {
                let time_sent = tx_info.time_sent;
                *max_acked_tx_time = (*max_acked_tx_time).max(Some(time_sent));

                // Track CCA info from most recent packet
                if cca_args
                    .as_ref()
                    .map_or(true, |(prev_time, _)| *prev_time < time_sent)
                {
                    *cca_args = Some((time_sent, tx_info.cc_info));
                }

                *bytes_acked += tx_info.sent_bytes as usize;
            }

            tracing::debug!(packet_number = num.as_u64(), "Packet ACKed");

            queue_range.push_back(entry);
        }

        queue.prepend(&mut queue_range);
    }

    let _ = acked.send(queue);
}

/// Detect lost packets using QUIC loss detection algorithm and queue for retransmission
fn detect_and_retransmit_lost_packets<Rand>(
    context: &mut socket::channel::PathContext<s2n_quic_dc::crypto::awslc::seal::Application>,
    max_acked_pn: VarInt,
    max_tx_time: s2n_quic_core::time::Timestamp,
    lost: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    now: s2n_quic_core::time::Timestamp,
    random: &mut Rand,
) where
    Rand: random::Generator,
{
    // Calculate loss delay using QUIC loss detection algorithm
    let loss_delay = {
        let rtt = context
            .rtt_estimator
            .smoothed_rtt()
            .max(context.rtt_estimator.latest_rtt());
        // kTimeThreshold is typically 9/8 per RFC
        let time_threshold = rtt + rtt / 8;
        // kGranularity is typically 1ms
        time_threshold.max(Duration::from_millis(1))
    };

    let loss_time = max_tx_time.checked_sub(loss_delay);

    // Packet number threshold: packets <= max_acked_pn - 3 are considered lost
    let pn_threshold = max_acked_pn.checked_sub(VarInt::from_u8(3));

    // Find the maximum lost packet number
    let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
    let lost_max = pn_threshold.map(|v| PacketNumberSpace::Initial.new_packet_number(v));

    let mut lost_queue = Queue::new();

    // Remove lost packets and batch them for retransmission (enables GSO)
    if let Some(lost_max) = lost_max {
        let range = PacketNumberRange::new(lost_min, lost_max);
        let mut lost_count = 0usize;
        for (num, mut entry) in context.packet_number_map.remove_range(range) {
            // Update CCA for packet loss
            let tx_info = entry.transmission_info.take().unwrap();

            tracing::trace!(
                pn = num.as_u64(),
                max_acked = max_acked_pn.as_u64(),
                time_sent = ?tx_info.time_sent,
                "Packet lost by PN threshold"
            );

            context
                .cca
                .on_packet_lost(tx_info.sent_bytes as u32, tx_info.cc_info, random, now);

            lost_count += 1;
            lost_queue.push_back(entry);
        }

        if lost_count > 0 {
            tracing::debug!(
                lost_count,
                max_acked = max_acked_pn.as_u64(),
                threshold = pn_threshold.map(|v| v.as_u64()),
                rtt = ?context.rtt_estimator.smoothed_rtt(),
                "Loss detection triggered"
            );
        }
    };

    let _ = lost.send(lost_queue);

    // TODO also do time-based loss detection
    // for (num, packet) in context.packet_number_map.iter() {
    //     if let Some(tx_info) = &packet.transmission_info {
    //         // A packet is considered lost if it meets either condition:
    //         // 1. Time threshold: sent before loss_time
    //         // 2. Packet number threshold: packet number <= max_acked_pn - 3
    //         let lost_by_time = loss_time.map_or(false, |loss_time| tx_info.time_sent <= loss_time);
    //         let lost_by_pn =
    //             pn_threshold.map_or(false, |threshold| num.as_u64() <= threshold.as_u64());

    //         if lost_by_time || lost_by_pn {
    //             lost_max = Some(num);
    //             continue;
    //         }
    //     }

    //     break;
    // }
}

// ── Datagram Processing ────────────────────────────────────────────────────

enum ProcessError {
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
}

/// Process a received datagram packet for ACK generation.
///
/// This authenticates the packet by decrypting it, filters duplicates, and
/// updates the peer state to generate ACKs. Returns the decrypted packet on success.
fn process_datagram<Clk>(
    mut packet: Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
    peer_cache: &mut PeerStateCache,
    path_secret_map: &path::secret::Map,
    clock: &Clk,
) -> Result<Entry<packet::datagram::decoder::Packet<descriptor::Filled>>, ProcessError>
where
    Clk: s2n_quic_core::time::Clock + ?Sized,
{
    let credentials = *packet.credentials();

    let idle_timeout = peer_cache.idle_timeout;

    // Get or create peer state
    let mut control_out = Vec::new();
    let Some(peer_state) =
        peer_cache.get_or_insert(&credentials, path_secret_map, clock, &mut control_out)
    else {
        return Err(ProcessError::PeerStateLookup {
            credentials,
            control_out,
        });
    };

    let packet_number = packet.packet_number();

    // First, authenticate the packet by decrypting in place
    // This ensures we only process authentic packets
    if packet.decrypt_in_place(&peer_state.opener).is_err() {
        return Err(ProcessError::Decryption {
            credentials,
            packet_number,
        });
    }

    // Now that we know the packet is authentic, check for duplicates
    if peer_state
        .ack_space
        .filter
        .on_packet_number(packet_number)
        .is_err()
    {
        return Err(ProcessError::Duplicate {
            credentials,
            packet_number,
        });
    }

    // Update activity timestamp
    peer_state.update_activity(clock, idle_timeout);

    // Record the packet for ACK and track its ECN marking so we can report
    // the cumulative ECN counts back to the sender in the next ACK frame.
    let ecn = packet.storage().ecn();
    peer_state.ecn_counts.increment(ecn);
    peer_state
        .ack_space
        .on_packet_received(packet_number, clock.get_time());

    // Mark ACK as queued
    peer_state.transmission_state = AckTransmissionState::Queued;

    Ok(packet)
}

enum ProcessControlError {
    PeerStateLookup {
        credentials: Credentials,
        control_out: Vec<u8>,
    },
    Verification {
        credentials: Credentials,
        packet_number: VarInt,
    },
}

/// Process a received control packet for ACK processing.
///
/// This authenticates the packet by verifying its MAC tag.
fn process_control<Clk>(
    packet: Entry<packet::control::decoder::Packet<descriptor::Filled>>,
    peer_cache: &mut PeerStateCache,
    path_secret_map: &path::secret::Map,
    clock: &Clk,
) -> Result<Entry<packet::control::decoder::Packet<descriptor::Filled>>, ProcessControlError>
where
    Clk: s2n_quic_core::time::Clock + ?Sized,
{
    let credentials = *packet.credentials();

    let idle_timeout = peer_cache.idle_timeout;

    // Get or create peer state
    let mut control_out = Vec::new();
    let Some(peer_state) =
        peer_cache.get_or_insert(&credentials, path_secret_map, clock, &mut control_out)
    else {
        return Err(ProcessControlError::PeerStateLookup {
            credentials,
            control_out,
        });
    };

    let packet_number = packet.packet_number();

    // Authenticate the packet by verifying the MAC tag
    if packet.verify(&peer_state.opener).is_err() {
        return Err(ProcessControlError::Verification {
            credentials,
            packet_number,
        });
    }

    // Update activity timestamp
    peer_state.update_activity(clock, idle_timeout);

    // Record the ECN marking from this control packet so the ACK frames we
    // send back include accurate ECN counts covering both datagram and control
    // packet types that share the same ack_space.
    let ecn = packet.storage().ecn();
    peer_state.ecn_counts.increment(ecn);

    // Record the packet for ACK
    peer_state
        .ack_space
        .on_packet_received(packet_number, clock.get_time());

    Ok(packet)
}

// ── Peer State Management ──────────────────────────────────────────────────

/// Cached crypto state and ACK tracking for a single peer
struct PeerState {
    /// Path secret entry for this peer
    path_entry: Arc<PathSecretEntry>,
    /// Opener for decrypting datagrams from this peer
    /// This gets updated when we see a new key_id from the same peer
    opener: s2n_quic_dc::crypto::awslc::open::Application,
    /// The key_id this opener corresponds to
    current_key_id: VarInt,
    /// ACK space for tracking received packets (spans all key_ids for this peer)
    ack_space: s2n_quic_dc::stream::recv::ack::Space,
    /// Accumulated ECN counts for received packets, reported back to the sender
    /// in each ACK frame so the sender can validate ECN support and detect congestion.
    ecn_counts: EcnCounts,
    /// Timer for idle timeout
    idle_timer: s2n_quic_core::time::Timer,
    /// Last activity timestamp
    last_activity: s2n_quic_core::time::Timestamp,
    /// Transmission state for ACKs
    transmission_state: AckTransmissionState,
}

/// Simplified ACK transmission state for datagrams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckTransmissionState {
    Idle,
    Queued,
}

impl PeerState {
    fn new<Clk>(
        path_entry: Arc<PathSecretEntry>,
        opener: s2n_quic_dc::crypto::awslc::open::Application,
        key_id: VarInt,
        clock: &Clk,
        idle_timeout: Duration,
    ) -> Self
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let now = clock.get_time();
        let mut idle_timer = s2n_quic_core::time::Timer::default();
        idle_timer.set(now + idle_timeout);

        Self {
            path_entry,
            opener,
            current_key_id: key_id,
            ack_space: Default::default(),
            ecn_counts: Default::default(),
            idle_timer,
            last_activity: now,
            transmission_state: AckTransmissionState::Idle,
        }
    }

    fn update_activity<Clk>(&mut self, clock: &Clk, idle_timeout: Duration)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let now = clock.get_time();
        self.last_activity = now;
        self.idle_timer.set(now + idle_timeout);
    }

    fn is_expired<Clk>(&mut self, clock: &Clk) -> bool
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.idle_timer.poll_expiration(clock.get_time()).is_ready()
    }

    fn should_transmit(&self) -> bool {
        self.transmission_state == AckTransmissionState::Queued
    }

    /// Generate an ACK control packet for this peer
    fn generate_ack_packet<Clk>(
        &mut self,
        clock: &Clk,
        routing_info: RoutingInfo,
    ) -> Option<PartialDatagram>
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        // Generate ACK frame from the ACK space.  Only include ECN counts when
        // at least one ECN-marked packet has been seen; this avoids forcing the
        // wider ACK-with-ECN frame encoding (which drops more ACK ranges to fit
        // the MTU) when the counts would all be zero anyway.
        let mtu = 1400u16;
        let (ack_frame, encoding_size) = self
            .ack_space
            .encoding(VarInt::ZERO, self.ecn_counts.as_option(), mtu, clock);

        let ack_frame = ack_frame?;

        // Allocate a Vec<u8> for the encoded frame
        let mut buffer = vec![0u8; encoding_size.as_u64() as usize];
        let mut encoder_buf = EncoderBuffer::new(&mut buffer);
        encoder_buf.encode(&ack_frame);

        let control_data = buffer.into();

        // Create and return the control packet
        Some(PartialDatagram::new_control(
            routing_info,
            control_data,
            self.path_entry.clone(),
        ))
    }
}

/// Per-worker peer state cache
struct PeerStateCache {
    /// Map from credentials to peer state
    peers: std::collections::HashMap<Credentials, PeerState>,
    /// Idle timeout for peer states
    idle_timeout: Duration,
}

impl PeerStateCache {
    fn new(idle_timeout: Duration) -> Self {
        Self {
            peers: std::collections::HashMap::new(),
            idle_timeout,
        }
    }

    fn get_or_insert<Clk>(
        &mut self,
        credentials: &Credentials,
        path_secret_map: &s2n_quic_dc::path::secret::map::Map,
        clock: &Clk,
        control_out: &mut Vec<u8>,
    ) -> Option<&mut PeerState>
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        // Use entry API for single hash lookup
        Some(match self.peers.entry(*credentials) {
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
            hash_map::Entry::Vacant(entry) => {
                // Slow path: derive opener from map
                let (opener, path_entry) = path_secret_map.opener_for_credentials(
                    credentials,
                    None, // queue_id is None for datagrams
                    control_out,
                )?;

                entry.insert(PeerState::new(
                    path_entry,
                    opener,
                    credentials.key_id,
                    clock,
                    self.idle_timeout,
                ))
            }
        })
    }

    fn cleanup_expired<Clk>(&mut self, clock: &Clk)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.peers.retain(|_, state| !state.is_expired(clock));
    }
}

// ── Receive Pipeline Components ────────────────────────────────────────────

/// Helper to assert a type implements Receiver<T>
fn assert_receiver<T>(_r: &impl channel::Receiver<T>) {}

/// Packet router that routes packets to channels for processing
struct ChannelRouter<D, C> {
    datagram_tx: D,
    control_tx: C,
}

impl<D, C> Router for ChannelRouter<D, C>
where
    D: channel::UnboundedSender<Entry<packet::datagram::decoder::Packet<descriptor::Filled>>>,
    C: channel::UnboundedSender<Entry<packet::control::decoder::Packet<descriptor::Filled>>>,
{
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        packet: packet::datagram::decoder::Packet<descriptor::Filled>,
    ) {
        let _ = self.datagram_tx.send(packet.into());
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::datagram::decoder::Packet<&mut [u8]>,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        packet: packet::control::decoder::Packet<descriptor::Filled>,
    ) {
        let _ = self.control_tx.send(packet.into());
    }

    #[inline]
    fn handle_control_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::control::decoder::Packet<&mut [u8]>,
    ) {
    }
}

// ── Send Pipeline Components ───────────────────────────────────────────────

/// Simple batch sender that drains a channel and sends to a socket
pub async fn batch_sender<S, R>(socket: S, rx: R)
where
    S: socket::send::Socket,
    R: channel::Receiver<Entry<Batch>>,
{
    let local_addr = socket.local_addr().unwrap();

    let rx = channel::SocketSender::new(rx, socket);
    let rx = channel::InspectErr::new(rx, |(err, _entry)| {
        tracing::warn!("socket send error: {err}");
    });

    // Map to () after successful send
    let rx = channel::Map::new(rx, |batch| {
        debug_assert!(batch.encoded.is_some(), "batch should have encoded data");
        debug_assert!(batch.datagrams.is_empty(), "datagrams should be consumed");
        // Drop the batch
    });

    rx.drain().await;

    info!(%local_addr, "Socket sender shutting down");
}

/// Per-socket path context storage
///
/// Each send socket maintains its own packet number space, CCA state, and RTT estimator.
struct SocketPathContexts {
    /// Map from credentials ID to path contexts
    contexts: RefCell<
        std::collections::HashMap<
            credentials::Id,
            Rc<
                RefCell<
                    socket::channel::PathContext<s2n_quic_dc::crypto::awslc::seal::Application>,
                >,
            >,
        >,
    >,
    max_datagram_size: u16,
}

impl SocketPathContexts {
    fn new(max_datagram_size: u16) -> Self {
        Self {
            contexts: RefCell::new(std::collections::HashMap::new()),
            max_datagram_size,
        }
    }

    /// Get or create a path context for the given path entry
    fn get_or_insert(
        &self,
        entry: &Arc<PathSecretEntry>,
    ) -> Rc<RefCell<socket::channel::PathContext<s2n_quic_dc::crypto::awslc::seal::Application>>>
    {
        let credentials_id = *entry.id();

        let mut contexts = self.contexts.borrow_mut();
        if let Some(context) = contexts.get(&credentials_id) {
            return context.clone();
        }

        // Create new context - call reusable_sealer() only once
        let (sealer, credentials) = entry.reusable_sealer();

        // Create a new CCA controller
        let cca = congestion::Controller::new(self.max_datagram_size);

        // Create a new RTT estimator
        let rtt_estimator = s2n_quic_core::recovery::RttEstimator::new(Duration::from_millis(2));

        // Create a new packet number map
        let packet_number_map = s2n_quic_core::packet::number::Map::default();

        let context = socket::channel::PathContext {
            path_secret_entry: entry.clone(),
            sealer,
            credentials,
            next_packet_number: VarInt::ZERO,
            cca,
            rtt_estimator,
            packet_number_map,
        };

        let context = Rc::new(RefCell::new(context));
        contexts.insert(credentials_id, context.clone());
        context
    }
}

/// Simple cached path context resolver for demo purposes
///
/// In production, this should use proper LRU caching and idle timers.
pub struct SimplePathContextResolver {
    socket_contexts: Rc<SocketPathContexts>,
}

impl SimplePathContextResolver {
    fn new(socket_contexts: Rc<SocketPathContexts>) -> Self {
        Self { socket_contexts }
    }
}

impl socket::channel::PathContextResolver for SimplePathContextResolver {
    type Sealer = s2n_quic_dc::crypto::awslc::seal::Application;

    fn resolve(
        &self,
        entry: &Arc<PathSecretEntry>,
    ) -> Option<Rc<RefCell<socket::channel::PathContext<Self::Sealer>>>> {
        Some(self.socket_contexts.get_or_insert(entry))
    }
}

// ── Socket Creation ────────────────────────────────────────────────────────

/// Creates send sockets with GSO support
pub fn create_send_sockets(
    num_sockets: usize,
    bind_addr: SocketAddr,
    disable_gso: bool,
) -> io::Result<Vec<Gso<BusyPoll<std::net::UdpSocket>>>> {
    let gso = features::Gso::default();
    if disable_gso {
        gso.disable();
    }

    let mut sockets = Vec::with_capacity(num_sockets);
    for _ in 0..num_sockets {
        let mut opts = Options::default();
        opts.addr = bind_addr;
        opts.blocking = false;
        opts.send_buffer = Some(20 * 1024 * 1024); // 20MB per socket
        opts.recv_buffer = Some(0);
        let socket = opts.build_udp()?;

        // Wrap with busy poll support then GSO
        let socket = BusyPoll(socket);
        let socket = Gso(socket, gso.clone());
        sockets.push(socket);
    }

    Ok(sockets)
}

/// Creates receive sockets with REUSEPORT for load balancing
pub fn create_recv_sockets(
    num_sockets: usize,
    bind_addr: SocketAddr,
) -> io::Result<Vec<BusyPoll<std::net::UdpSocket>>> {
    let mut sockets = Vec::with_capacity(num_sockets);

    // First socket - binds the address (will get ephemeral port if port is 0)
    let mut opts = Options::default();
    opts.addr = bind_addr;
    opts.reuse_address = true;
    opts.reuse_port = ReusePort::AfterBind;
    opts.gro = true;
    opts.blocking = false;
    opts.recv_buffer = Some(200 * 1024 * 1024);
    opts.send_buffer = Some(0);
    let first_socket = opts.build_udp()?;
    sockets.push(BusyPoll(first_socket));

    // If we have more than one socket, use REUSEPORT to share the port
    if num_sockets > 1 {
        // Get the actual bound address from the first socket
        let bound_addr = sockets[0].0.local_addr()?;

        opts.reuse_port = ReusePort::BeforeBind;

        // Remaining sockets share the same port
        opts.addr = bound_addr;
        for _ in 1..num_sockets {
            sockets.push(BusyPoll(opts.build_udp()?));
        }
    }

    Ok(sockets)
}

/// Complete bidirectional pipeline setup
pub struct Pipeline {
    /// Input sender for the wheel (producers send batches here)
    pub wheel_input_tx: intrusive_queue::sync::Sender<Batch>,
}

pub struct PipelineConfig<'a> {
    pub packet_size: u16,
    pub overall_send_rate: Rate,
    pub per_socket_send_rate: Rate,
    pub busy_poll: &'a s2n_quic_dc::busy_poll::Pool,
    pub clock: BusyPollClock<TokioClock>,
    pub send_pool: pool::Pool,
    pub recv_pool: pool::Pool,
    pub counters: CounterRegistry,
    /// Path secret map for looking up crypto state by credentials
    pub path_secret_map: s2n_quic_dc::path::secret::map::Map,
}

struct SendSocketInfo<S> {
    sender_id: usize,
    socket: S,
    batch_rx: intrusive_queue::sync::Receiver<Batch>,
}

struct RecvSocketInfo<S> {
    socket_id: usize,
    socket: S,
}

struct Worker<SendSocket, RecvSocket> {
    id: usize,
    send_sockets: Vec<SendSocketInfo<SendSocket>>,
    recv_sockets: Vec<RecvSocketInfo<RecvSocket>>,
    // One datagram handler per worker
    datagram_rx:
        intrusive_queue::sync::Receiver<packet::datagram::decoder::Packet<descriptor::Filled>>,
    // One control packet handler per worker (handles all send sockets on this worker)
    control_rx:
        intrusive_queue::sync::Receiver<packet::control::decoder::Packet<descriptor::Filled>>,
}

/// Sets up a complete bidirectional pipeline with send and receive paths
///
/// Send sockets: wheel -> encoder -> paced sender -> socket
/// Receive sockets: socket -> decoder -> router (for ACKs, etc)
///
/// The receive path can route parsed packets back to specific send workers
/// using the WorkerId routing info.
pub fn setup_pipeline<SendSocket, RecvSocket, G>(
    config: PipelineConfig,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Pipeline
where
    SendSocket: socket::send::Socket + Send + Sync + 'static,
    RecvSocket: socket::recv::Socket + Send + 'static,
    G: random::Generator,
{
    let PipelineConfig {
        packet_size,
        overall_send_rate,
        per_socket_send_rate,
        busy_poll,
        clock,
        send_pool,
        recv_pool,
        path_secret_map,
        counters,
    } = config;

    let num_send_sockets = send_sockets.len();

    // Create counter registry for instrumentation
    counters.spawn_reporter(Duration::from_secs(1));

    // Get the control port from the first receive socket (all receive sockets share the same port with REUSEPORT)
    let source_control_port = recv_sockets
        .get(0)
        .and_then(|s| s.local_addr().ok())
        .map(|addr| addr.port())
        .unwrap_or(0);

    // Create intrusive queue channels for socket senders (need Send to cross thread boundaries)
    let mut socket_senders = Vec::with_capacity(num_send_sockets);
    let mut socket_receivers = Vec::with_capacity(num_send_sockets);

    for _ in 0..num_send_sockets {
        let (tx, rx) = intrusive_queue::sync::new();
        socket_senders.push(tx);
        socket_receivers.push(rx);
    }

    // Create channel for wheel input from generators
    let (wheel_input_tx, wheel_input_rx) = intrusive_queue::sync::new();

    // Create the timing wheel that wraps the input receiver
    let wheel_timer = clock.timer();
    let wheel: Wheel<Batch, _, _, 1> = Wheel::new(wheel_input_rx, wheel_timer);

    // Spawn wheel ticker + distributor on busy poll worker 0
    busy_poll[0].spawn_local({
        let clock = clock.clone();
        let socket_senders = socket_senders.clone();
        move |mut spawner| {
            // Task 1: Pump wheel output into a channel for distribution
            let (wheel_output_tx, wheel_output_rx) = intrusive_queue::unsync::new();

            spawner.spawn(channel::pump(wheel, wheel_output_tx));

            // Task 2: Overall bandwidth limiter + round robin distributor
            let wheel_rx = FlattenQueue::new(wheel_output_rx);
            let wheel_rx = Paced::new(wheel_rx, clock.clone(), overall_send_rate);
            let wheel_rx = Reporter::new(wheel_rx, clock.clone(), true);

            spawner.spawn(channel::round_robin(wheel_rx, socket_senders));
            info!("Finished spawning wheel tasks");
        }
    });

    // Create error channel for failed batches
    let (error_tx, error_rx) = intrusive_queue::sync::new();

    let path_idle_timeout = Duration::from_secs(60);

    // Group send sockets, recv sockets, and worker channels by busy poll worker
    let num_workers = busy_poll.len() - 1;

    // Create worker channels (one datagram and one control channel per worker)
    let mut workers = Vec::with_capacity(num_workers);
    let mut datagram_receiver_tx = Vec::with_capacity(num_workers);
    let mut control_packet_tx = Vec::with_capacity(num_workers);
    for id in 0..num_workers {
        let (datagram_tx, datagram_rx) = intrusive_queue::sync::new();
        let (control_tx, control_rx) = intrusive_queue::sync::new();
        datagram_receiver_tx.push(datagram_tx);
        control_packet_tx.push(control_tx);
        workers.push(Worker {
            id,
            send_sockets: Vec::new(),
            recv_sockets: Vec::new(),
            datagram_rx,
            control_rx,
        });
    }

    // Build sender_id to worker_id mapping (for control packet routing)
    let mut sender_id_to_worker = Vec::with_capacity(num_send_sockets);

    // Distribute send sockets across workers
    for (sender_id, (socket, batch_rx)) in
        send_sockets.into_iter().zip(socket_receivers).enumerate()
    {
        let worker_idx = sender_id % num_workers;
        sender_id_to_worker.push(worker_idx);
        workers[worker_idx].send_sockets.push(SendSocketInfo {
            sender_id,
            socket,
            batch_rx,
        });
    }

    let sender_id_to_worker = Arc::new(sender_id_to_worker);

    // Distribute recv sockets across workers
    for (socket_id, socket) in recv_sockets.into_iter().enumerate() {
        let worker_idx = socket_id % num_workers;
        workers[worker_idx]
            .recv_sockets
            .push(RecvSocketInfo { socket_id, socket });
    }

    // Spawn all tasks for each busy poll worker
    for worker in workers.into_iter() {
        let busy_worker_idx = 1 + worker.id;
        let clock = clock.clone();
        let send_pool = send_pool.clone();
        let recv_pool = recv_pool.clone();
        let datagram_receiver_tx = datagram_receiver_tx.clone();
        let control_packet_tx = control_packet_tx.clone();
        let error_tx = error_tx.clone();
        let path_secret_map = path_secret_map.clone();
        let wheel_input_tx = wheel_input_tx.clone();
        let counters = counters.clone();
        let control_generator = create_rand();
        let sender_id_to_worker = sender_id_to_worker.clone();

        busy_poll[busy_worker_idx].spawn_local(move |mut spawner| {
            // Create per-worker state
            let shared_peer_cache = Rc::new(RefCell::new(PeerStateCache::new(path_idle_timeout)));

            // Map sender_id to path contexts for control packet processing
            // Control worker needs to look up contexts for all sockets on this worker
            let sender_contexts: Rc<RefCell<HashMap<usize, Rc<SocketPathContexts>>>> =
                Rc::new(RefCell::new(HashMap::new()));

            // Create channels for ACKed and lost packets (shared across control worker)
            let (mut acked_tx, acked_rx) = channel::intrusive_queue::unsync::new();
            let (mut lost_tx, lost_rx) = channel::intrusive_queue::unsync::new();

            // Spawn ACKed packet handler with batched completion notifications
            spawner.spawn({
                let acked_counter = counters.register("acked_packets");
                async move {
                    let rx = channel::FlattenQueue::new(acked_rx);
                    let rx = channel::Inspect::new(rx, |_entry: &Entry<PartialDatagram>| {
                        acked_counter.fetch_add(1, Ordering::Relaxed);
                    });
                    let rx = channel::CompletionBatcher::new(rx);
                    rx.drain().await;
                }
            });

            // Spawn lost packet handler - batch and retransmit
            spawner.spawn({
                let wheel_input_tx = wheel_input_tx.clone();
                let lost_datagrams_counter = counters.register("lost_datagrams");
                let lost_control_counter = counters.register("lost_control");
                async move {
                    // Flatten queues of lost packets into individual entries
                    let rx = channel::FlattenQueue::new(lost_rx);

                    // Filter out control packets - they don't need retransmission
                    let rx =
                        channel::FilterMap::new(rx, |entry: Entry<PartialDatagram>| {
                            match entry.packet_type {
                                packet::datagram::partial::PacketType::Datagram { .. } => {
                                    lost_datagrams_counter.fetch_add(1, Ordering::Relaxed);
                                    Some(entry)
                                }
                                packet::datagram::partial::PacketType::Control { .. } => {
                                    lost_control_counter.fetch_add(1, Ordering::Relaxed);
                                    tracing::trace!("Skipping control packet retransmission");
                                    None
                                }
                            }
                        });

                    // Batch lost packets by peer address for efficient retransmission with GSO
                    let rx = channel::RetransmissionBatcher::new(rx);

                    // Inspect batches before retransmission
                    let rx = channel::Inspect::new(rx, |batch: &Entry<Batch>| {
                        tracing::debug!(
                            peer_addr = ?batch.meta.peer_addr,
                            count = batch.datagrams.len(),
                            "Retransmitting lost packets"
                        );
                    });

                    // Pump retransmission batches back into the wheel
                    channel::pump(rx, wheel_input_tx).await;
                }
            });

            // Spawn send socket tasks for each socket on this worker
            for socket_info in worker.send_sockets {
                let SendSocketInfo {
                    sender_id,
                    socket,
                    batch_rx,
                } = socket_info;

                // Create per-socket path contexts
                let socket_contexts = Rc::new(SocketPathContexts::new(packet_size));

                // Register this socket's contexts for control packet processing
                sender_contexts
                    .borrow_mut()
                    .insert(sender_id, socket_contexts.clone());

                let error_tx = error_tx.clone();
                let pool = send_pool.clone();
                let clock = clock.clone();
                let socket_send_counter = counters.register("socket_send");
                let local_addr = socket.local_addr().unwrap();
                let source_sender_id = VarInt::new(sender_id as u64).unwrap();

                // Create channel between Paced and PacketRegistrar
                let (paced_tx, paced_rx) = intrusive_queue::unsync::new();

                // Task 1: Encoder + PacketRegistrar + Paced -> pump to channel
                spawner.spawn({
                    let clock = clock.clone();
                    async move {
                        // Build the channel adapter pipeline with timing instrumentation
                        let rx = FlattenQueue::new(batch_rx);
                        let rx = channel::Timing::new(rx, "flatten");

                        let resolver = SimplePathContextResolver::new(socket_contexts);
                        let rx = channel::PathResolver::new(rx, resolver, error_tx);
                        let rx = channel::Timing::new(rx, "path_resolver");

                        let rx =
                            channel::Encoder::new(rx, pool, source_control_port, source_sender_id);
                        let rx = channel::Timing::new(rx, "encoder");

                        let rx = channel::PacketRegistrar::new(rx, clock.clone());
                        let rx = channel::Timing::new(rx, "packet_registrar");

                        channel::pump(rx, paced_tx).await;
                        tracing::info!(sender_id, "Paced pump shutting down");
                    }
                });

                // Task 2: Channel -> Socket
                spawner.spawn(async move {
                    let rx = paced_rx;

                    let rx = Paced::new(rx, clock.clone(), per_socket_send_rate);
                    let rx = channel::Timing::new(rx, "paced");

                    // Count batches right before socket transmission
                    let rx = channel::Inspect::new(rx, move |batch: &Entry<Batch>| {
                        socket_send_counter.fetch_add(1, Ordering::Relaxed);
                    });

                    batch_sender(socket, rx).await;
                    info!(sender_id, ?local_addr, "Socket sender shutting down");
                });
            }

            // Spawn single control worker task for all send sockets on this worker
            {
                let worker_id = worker.id;
                let control_rx = worker.control_rx;
                let path_secret_map = path_secret_map.clone();
                let shared_peer_cache = shared_peer_cache.clone();
                let sender_contexts = sender_contexts.clone();
                let clock = clock.clone();
                let mut generator = control_generator;
                let recv_control_counter = counters.register("recv_control");

                spawner.spawn(async move {
                    // Process control packets for ACK frame processing
                    let rx = Map::new(control_rx, {
                        let clock = clock.clone();
                        let recv_control_counter = recv_control_counter.clone();
                        move |packet: Entry<
                            packet::control::decoder::Packet<descriptor::Filled>,
                        >| {
                            recv_control_counter.fetch_add(1, Ordering::Relaxed);
                            process_control(
                                packet,
                                &mut shared_peer_cache.borrow_mut(),
                                &path_secret_map,
                                &clock,
                            )
                        }
                    });
                    assert_receiver(&rx);

                    let rx = InspectErr::new(rx, move |err| match err {
                        ProcessControlError::PeerStateLookup {
                            credentials,
                            control_out,
                        } => {
                            // TODO transmit this
                            let _ = control_out;
                            tracing::warn!(
                                worker_id,
                                ?credentials,
                                "Failed to get or create peer state for control packet"
                            );
                        }
                        ProcessControlError::Verification {
                            credentials,
                            packet_number,
                        } => {
                            tracing::debug!(
                                worker_id,
                                ?credentials,
                                pn = packet_number.as_u64(),
                                "Failed to verify control packet - authentication failed"
                            );
                        }
                    });
                    assert_receiver(&rx);

                    // Process ACK frames and update send state using sender_contexts
                    let rx = Map::new(rx, {
                        let clock = clock.clone();
                        move |mut packet| {
                            // Extract dest_sender_id from routing info
                            let dest_sender_id = match packet.routing_info() {
                                RoutingInfo::SenderId { sender_id } => sender_id.as_u64() as usize,
                                _ => {
                                    tracing::warn!(
                                        worker_id,
                                        "Control packet without SenderId - cannot process ACK"
                                    );
                                    return;
                                }
                            };

                            // Look up the socket's path contexts
                            let sender_contexts_ref = sender_contexts.borrow();
                            let Some(socket_contexts) = sender_contexts_ref.get(&dest_sender_id)
                            else {
                                tracing::warn!(
                                    worker_id,
                                    dest_sender_id,
                                    "No socket contexts for sender_id"
                                );
                                return;
                            };

                            // Get the path context for this peer
                            let ack_credentials = *packet.credentials();
                            let credentials_id = ack_credentials.id;

                            let contexts = socket_contexts.contexts.borrow_mut();
                            let Some(context_rc) = contexts.get(&credentials_id) else {
                                tracing::warn!(
                                    worker_id,
                                    dest_sender_id,
                                    ?credentials_id,
                                    "No path context for credentials"
                                );
                                return;
                            };

                            let mut context = context_rc.borrow_mut();
                            process_control_frames(
                                worker_id,
                                &mut packet,
                                &mut context,
                                &mut acked_tx,
                                &mut lost_tx,
                                &clock,
                                &mut generator,
                            );

                            // TODO: Clear ACK ranges for packets that have been acknowledged
                            // When we receive an ACK from the peer, we should call
                            // peer_state.ack_space.on_largest_delivered_packet(largest_delivered)
                            // to prevent re-sending ACKs for packets the peer has confirmed.
                            // This requires access to shared_peer_cache here.
                        }
                    });
                    assert_receiver::<()>(&rx);

                    rx.drain().await;
                    tracing::info!(worker_id, "Control worker shutting down");
                });
            }

            // Spawn ACK worker (datagram processor) for this worker
            {
                let worker_id = worker.id;
                let datagram_rx = worker.datagram_rx;
                let path_secret_map = path_secret_map.clone();
                let clock = clock.clone();
                let wheel_input_tx = wheel_input_tx.clone();
                let recv_data_counter = counters.register("recv_data");
                let sent_ack_counter = counters.register("sent_ack");
                let ack_to_wheel_counter = counters.register("ack_to_wheel");

                spawner.spawn(async move {
                    // Process datagrams for ACK generation
                    let rx = datagram_rx;

                    let rx = Map::new(rx, {
                        let recv_data_counter = recv_data_counter.clone();
                        let shared_peer_cache = shared_peer_cache.clone();
                        let path_secret_map = path_secret_map.clone();
                        let clock = clock.clone();
                        move |packet: Entry<
                            packet::datagram::decoder::Packet<descriptor::Filled>,
                        >| {
                            recv_data_counter.fetch_add(1, Ordering::Relaxed);
                            process_datagram(
                                packet,
                                &mut shared_peer_cache.borrow_mut(),
                                &path_secret_map,
                                &clock,
                            )
                        }
                    });
                    let rx = InspectErr::new(rx, move |err| match err {
                        ProcessError::PeerStateLookup {
                            credentials,
                            control_out,
                        } => {
                            // TODO transmit this
                            let _ = control_out;
                            tracing::warn!(
                                worker_id,
                                ?credentials,
                                "Failed to get or create peer state"
                            );
                        }
                        ProcessError::Decryption {
                            credentials,
                            packet_number,
                        } => {
                            tracing::debug!(
                                worker_id,
                                ?credentials,
                                pn = packet_number.as_u64(),
                                "Failed to decrypt packet - authentication failed"
                            );
                        }
                        ProcessError::Duplicate {
                            credentials,
                            packet_number,
                        } => {
                            tracing::trace!(
                                worker_id,
                                ?credentials,
                                pn = packet_number.as_u64(),
                                "Duplicate packet filtered"
                            );
                        }
                    });

                    // Generate ACK packets
                    let rx = Map::new(rx, {
                        let shared_peer_cache = shared_peer_cache.clone();
                        let sent_ack_counter = sent_ack_counter.clone();
                        move |packet: Entry<
                            packet::datagram::decoder::Packet<descriptor::Filled>,
                        >| {
                            let credentials = *packet.credentials();
                            let mut peer_addr = packet.storage().remote_address().get();
                            let source_control_port = packet.meta().source_control_port();
                            if source_control_port > 0 {
                                peer_addr.set_port(source_control_port);
                            }

                            let routing_info = packet.routing_info();
                            let Some(source_sender_id) = routing_info.source_sender_id() else {
                                return Err("packet does not include a source_sender_id");
                            };

                            // Get peer state and generate ACK
                            let mut peer_cache_ref = shared_peer_cache.borrow_mut();
                            let Some(peer_state) = peer_cache_ref.peers.get_mut(&credentials)
                            else {
                                return Err("missing credentials for peer");
                            };

                            // Generate ACK control packet with dest_sender_id for routing back
                            let routing_info = RoutingInfo::SenderId {
                                sender_id: source_sender_id,
                            };
                            let Some(ack_packet) =
                                peer_state.generate_ack_packet(&clock, routing_info)
                            else {
                                return Err("ACK not needed");
                            };

                            // Mark ACK as transmitted and reset state
                            // peer_state.ack_space.on_transmit(&clock);
                            peer_state.transmission_state = AckTransmissionState::Idle;

                            sent_ack_counter.fetch_add(1, Ordering::Relaxed);

                            // Create a batch for the ACK control packet
                            let mut batch =
                                s2n_quic_dc::datagram::batch::Batch::new(None, peer_addr.into());
                            batch.datagrams.push_back(ack_packet.into());

                            Ok(Entry::new(batch))
                        }
                    });

                    let rx = InspectErr::new(rx, |err| {
                        tracing::warn!(worker_id, err, "ACK worker error");
                    });

                    // Count ACKs right before pumping to wheel
                    let rx = channel::Inspect::new(rx, move |_batch: &Entry<Batch>| {
                        ack_to_wheel_counter.fetch_add(1, Ordering::Relaxed);
                    });

                    channel::pump(rx, wheel_input_tx).await;

                    tracing::info!(worker_id, "ACK worker shutting down");
                });
            }

            // Spawn recv socket tasks for this worker
            for recv_socket_info in worker.recv_sockets {
                let RecvSocketInfo { socket_id, socket } = recv_socket_info;
                let clock = clock.clone();
                let recv_pool = recv_pool.clone();
                let datagram_receiver_tx = datagram_receiver_tx.clone();
                let control_packet_tx = control_packet_tx.clone();

                // Create local channels for datagram and control packet processing
                let (datagram_tx, datagram_rx) = intrusive_queue::unsync::new();
                let (control_tx, control_rx) = intrusive_queue::unsync::new();

                let local_addr = socket.local_addr().unwrap();

                // Spawn socket receiver task
                spawner.spawn(async move {
                    // Build the receive pipeline
                    let socket_rx = SocketReceiver::new(socket, recv_pool);
                    let socket_rx = InspectErr::new(socket_rx, |err| {
                        tracing::warn!(socket_id, %err, "Socket recv error");
                    });
                    let segments_rx = FlattenSegments::new(socket_rx);
                    let segments_rx = Reporter::new(segments_rx, clock.clone(), false);

                    let router = ChannelRouter {
                        datagram_tx,
                        control_tx,
                    };
                    let pipeline = RouterAdapter::new(segments_rx, router);

                    pipeline.drain().await;

                    info!(socket_id, ?local_addr, "Socket receiver shutting down");
                });

                // Spawn datagram router task
                let num_workers = datagram_receiver_tx.len();
                if num_workers.is_power_of_two() {
                    let mask = (num_workers - 1) as u64;
                    spawner.spawn(async move {
                        let rx = Map::new(
                            datagram_rx,
                            move |packet: Entry<
                                packet::datagram::decoder::Packet<descriptor::Filled>,
                            >| {
                                // Route based on credentials hash for consistent routing
                                // This ensures all packets for the same path go to the same ACK worker
                                let credentials = packet.credentials();
                                let hash = hash_credentials(credentials);
                                let worker_id = (hash & mask) as usize;
                                let _ = datagram_receiver_tx[worker_id].send_entry(packet);
                            },
                        );
                        rx.drain().await;
                        tracing::info!(socket_id, "Datagram router shutting down");
                    });
                } else {
                    spawner.spawn(async move {
                        let rx = Map::new(
                            datagram_rx,
                            move |packet: Entry<
                                packet::datagram::decoder::Packet<descriptor::Filled>,
                            >| {
                                // Route based on credentials hash for consistent routing
                                // This ensures all packets for the same path go to the same ACK worker
                                let credentials = packet.credentials();
                                let hash = hash_credentials(credentials);
                                let worker_id = (hash % num_workers as u64) as usize;
                                let _ = datagram_receiver_tx[worker_id].send_entry(packet);
                            },
                        );
                        rx.drain().await;
                        tracing::info!(socket_id, "Datagram router shutting down");
                    });
                }

                // Spawn control packet router task
                spawner.spawn({
                    let sender_id_to_worker = sender_id_to_worker.clone();
                    async move {
                        let rx = Map::new(
                            control_rx,
                            move |packet: Entry<
                                packet::control::decoder::Packet<descriptor::Filled>,
                            >| {
                                // Route based on SenderId in routing info
                                let sender_id = match packet.routing_info() {
                                    RoutingInfo::SenderId { sender_id } => {
                                        sender_id.as_u64() as usize
                                    }
                                    _ => {
                                        // No routing info, drop the packet
                                        tracing::warn!(
                                            socket_id,
                                            "Control packet without SenderId routing info"
                                        );
                                        return;
                                    }
                                };

                                // Look up which worker owns this sender_id
                                let Some(&worker_id) = sender_id_to_worker.get(sender_id) else {
                                    tracing::warn!(socket_id, sender_id, "Unknown sender_id");
                                    return;
                                };

                                let Some(sender) = control_packet_tx.get(worker_id) else {
                                    return;
                                };
                                let _ = sender.send_entry(packet);
                            },
                        );
                        rx.drain().await;
                        tracing::info!(socket_id, "Control router shutting down");
                    }
                });
            }
        });
    }

    // TODO: Spawn error handler to deal with failed batches
    drop(error_rx);

    Pipeline { wheel_input_tx }
}

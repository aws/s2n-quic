// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("path_secret_map:initialized")]
#[subject(endpoint)]
struct PathSecretMapInitialized {
    /// The capacity of the path secret map
    #[measure("capacity")]
    capacity: usize,
}

#[event("path_secret_map:uninitialized")]
#[subject(endpoint)]
struct PathSecretMapUninitialized {
    /// The capacity of the path secret map
    #[measure("capacity")]
    capacity: usize,

    /// The number of entries in the map
    #[measure("entries")]
    entries: usize,
}

#[event("path_secret_map:background_handshake_requested")]
#[subject(endpoint)]
/// Emitted when a background handshake is requested
struct PathSecretMapBackgroundHandshakeRequested<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,
}

#[event("path_secret_map:entry_replaced")]
#[subject(endpoint)]
/// Emitted when the entry is inserted into the path secret map
struct PathSecretMapEntryInserted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:entry_replaced")]
#[subject(endpoint)]
/// Emitted when the entry is considered ready for use
struct PathSecretMapEntryReady<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:entry_replaced")]
#[subject(endpoint)]
/// Emitted when an entry is replaced by a new one for the same `peer_address`
struct PathSecretMapEntryReplaced<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    new_credential_id: &'a [u8],

    #[snapshot("[HIDDEN]")]
    previous_credential_id: &'a [u8],
}

#[event("path_secret_map:unknown_path_secret_packet_sent")]
#[subject(endpoint)]
/// Emitted when an UnknownPathSecret packet was sent
struct UnknownPathSecretPacketSent<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:unknown_path_secret_packet_received")]
#[subject(endpoint)]
/// Emitted when an UnknownPathSecret packet was received
struct UnknownPathSecretPacketReceived<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:unknown_path_secret_packet_accepted")]
#[subject(endpoint)]
/// Emitted when an UnknownPathSecret packet was authentic and processed
struct UnknownPathSecretPacketAccepted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:unknown_path_secret_packet_rejected")]
#[subject(endpoint)]
/// Emitted when an UnknownPathSecret packet was rejected as invalid
struct UnknownPathSecretPacketRejected<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:unknown_path_secret_packet_dropped")]
#[subject(endpoint)]
/// Emitted when an UnknownPathSecret packet was dropped due to a missing entry
struct UnknownPathSecretPacketDropped<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:replay_definitely_detected")]
#[subject(endpoint)]
/// Emitted when credential replay was definitely detected
struct ReplayDefinitelyDetected<'a> {
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    key_id: u64,
}

#[event("path_secret_map:replay_potentially_detected")]
#[subject(endpoint)]
/// Emitted when credential replay was potentially detected, but could not be verified
/// due to a limiting tracking window
struct ReplayPotentiallyDetected<'a> {
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    key_id: u64,

    #[measure("gap")]
    gap: u64,
}

#[event("path_secret_map:replay_detected_packet_sent")]
#[subject(endpoint)]
/// Emitted when an ReplayDetected packet was sent
struct ReplayDetectedPacketSent<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:replay_detected_packet_received")]
#[subject(endpoint)]
/// Emitted when an ReplayDetected packet was received
struct ReplayDetectedPacketReceived<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:replay_detected_packet_accepted")]
#[subject(endpoint)]
/// Emitted when an StaleKey packet was authentic and processed
struct ReplayDetectedPacketAccepted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    key_id: u64,
}

#[event("path_secret_map:replay_detected_packet_rejected")]
#[subject(endpoint)]
/// Emitted when an ReplayDetected packet was rejected as invalid
struct ReplayDetectedPacketRejected<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:replay_detected_packet_dropped")]
#[subject(endpoint)]
/// Emitted when an ReplayDetected packet was dropped due to a missing entry
struct ReplayDetectedPacketDropped<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:stale_key_packet_sent")]
#[subject(endpoint)]
/// Emitted when an StaleKey packet was sent
struct StaleKeyPacketSent<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:stale_key_packet_received")]
#[subject(endpoint)]
/// Emitted when an StaleKey packet was received
struct StaleKeyPacketReceived<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:stale_key_packet_accepted")]
#[subject(endpoint)]
/// Emitted when an StaleKey packet was authentic and processed
struct StaleKeyPacketAccepted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:stale_key_packet_rejected")]
#[subject(endpoint)]
/// Emitted when an StaleKey packet was rejected as invalid
struct StaleKeyPacketRejected<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:stale_key_packet_dropped")]
#[subject(endpoint)]
/// Emitted when an StaleKey packet was dropped due to a missing entry
struct StaleKeyPacketDropped<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

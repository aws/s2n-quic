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

    #[measure("lifetime", Duration)]
    lifetime: core::time::Duration,
}

#[event("path_secret_map:background_handshake_requested")]
#[subject(endpoint)]
/// Emitted when a background handshake is requested
struct PathSecretMapBackgroundHandshakeRequested<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,
}

#[event("path_secret_map:entry_inserted")]
#[subject(endpoint)]
/// Emitted when the entry is inserted into the path secret map
struct PathSecretMapEntryInserted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],
}

#[event("path_secret_map:entry_ready")]
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

#[event("path_secret_map:id_entry_evicted")]
#[subject(endpoint)]
/// Emitted when an entry is evicted due to running out of space
struct PathSecretMapIdEntryEvicted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// Time since insertion of this entry
    #[measure("age", Duration)]
    age: core::time::Duration,
}

#[event("path_secret_map:addr_entry_evicted")]
#[subject(endpoint)]
/// Emitted when an entry is evicted due to running out of space
struct PathSecretMapAddressEntryEvicted<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// Time since insertion of this entry
    #[measure("age", Duration)]
    age: core::time::Duration,
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

#[event("path_secret_map:key_accepted")]
#[subject(endpoint)]
/// Emitted when a credential is accepted (i.e., post packet authentication and passes replay
/// check).
struct KeyAccepted<'a> {
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    key_id: u64,

    /// How far away this credential is from the leading edge of key IDs (after updating the edge).
    ///
    /// Zero if this shifted us forward.
    #[measure("gap")]
    gap: u64,

    /// How far away this credential is from the leading edge of key IDs (before updating the edge).
    ///
    /// Zero if this didn't change the leading edge.
    #[measure("forward_shift")]
    forward_shift: u64,
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

#[event("path_secret_map:address_cache_accessed")]
#[subject(endpoint)]
/// Emitted when the cache is accessed by peer address
///
/// This can be used to track cache hit ratios
struct PathSecretMapAddressCacheAccessed<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[bool_counter("hit")]
    hit: bool,
}

#[event("path_secret_map:address_cache_accessed_entry")]
#[subject(endpoint)]
/// Emitted when the cache is accessed by peer address successfully
///
/// Provides more information about the accessed entry.
struct PathSecretMapAddressCacheAccessedHit<'a> {
    #[nominal_counter("peer_address.protocol")]
    peer_address: SocketAddress<'a>,

    #[measure("age", Duration)]
    age: core::time::Duration,
}

#[event("path_secret_map:id_cache_accessed")]
#[subject(endpoint)]
/// Emitted when the cache is accessed by path secret ID
///
/// This can be used to track cache hit ratios
struct PathSecretMapIdCacheAccessed<'a> {
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    #[bool_counter("hit")]
    hit: bool,
}

#[event("path_secret_map:id_cache_accessed_entry")]
#[subject(endpoint)]
/// Emitted when the cache is accessed by path secret ID successfully
///
/// Provides more information about the accessed entry.
struct PathSecretMapIdCacheAccessedHit<'a> {
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    #[measure("age", Duration)]
    age: core::time::Duration,
}

#[event("path_secret_map:cleaner_cycled")]
#[subject(endpoint)]
/// Emitted when the cleaner task performed a single cycle
///
/// This can be used to track cache utilization
struct PathSecretMapCleanerCycled {
    /// The number of Path Secret ID entries left after the cleaning cycle
    #[measure("entries.id")]
    id_entries: usize,

    /// The number of Path Secret ID entries that were retired in the cycle
    #[measure("entries.id.retired")]
    id_entries_retired: usize,

    /// Count of entries accessed since the last cycle
    #[measure("entries.id.active")]
    id_entries_active: usize,

    /// The utilization percentage of the active number of entries after the cycle
    #[measure("entries.id.active.utilization", Percent)]
    id_entries_active_utilization: f32,

    /// The utilization percentage of the available number of entries after the cycle
    #[measure("entries.id.utilization", Percent)]
    id_entries_utilization: f32,

    /// The utilization percentage of the available number of entries before the cycle
    #[measure("entries.id.utilization.initial", Percent)]
    id_entries_initial_utilization: f32,

    /// The number of SocketAddress entries left after the cleaning cycle
    #[measure("entries.address")]
    address_entries: usize,

    /// Count of entries accessed since the last cycle
    #[measure("entries.address.active")]
    address_entries_active: usize,

    /// The utilization percentage of the active number of entries after the cycle
    #[measure("entries.address.active.utilization", Percent)]
    address_entries_active_utilization: f32,

    /// The number of SocketAddress entries that were retired in the cycle
    #[measure("entries.address.retired")]
    address_entries_retired: usize,

    /// The utilization percentage of the available number of address entries after the cycle
    #[measure("entries.address.utilization", Percent)]
    address_entries_utilization: f32,

    /// The utilization percentage of the available number of address entries before the cycle
    #[measure("entries.address.utilization.initial", Percent)]
    address_entries_initial_utilization: f32,

    /// The number of handshake requests that are pending after the cleaning cycle
    #[measure("handshake_requests")]
    handshake_requests: usize,

    /// The number of handshake requests that were retired in the cycle
    #[measure("handshake_requests.retired")]
    handshake_requests_retired: usize,

    /// How long we kept the handshake lock held (this blocks completing handshakes).
    #[measure("handshake_lock_duration", Duration)]
    handshake_lock_duration: core::time::Duration,

    /// Total duration of a cycle.
    #[measure("total_duration", Duration)]
    duration: core::time::Duration,
}

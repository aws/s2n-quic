// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("stream:write_flushed")]
#[checkpoint("latency")]
#[measure_counter("conn")]
pub struct StreamWriteFlushed {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    #[measure_counter("committed.conn", Bytes)]
    committed_len: usize,

    /// The amount of time it took to process the write request
    ///
    /// Note that this includes both any syscall and encryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:write_fin_flushed")]
#[checkpoint("latency")]
#[measure_counter("conn")]
pub struct StreamWriteFinFlushed {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    #[measure_counter("committed.conn", Bytes)]
    committed_len: usize,

    /// The amount of time it took to process the write request
    ///
    /// Note that this includes both any syscall and encryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:write_blocked")]
#[checkpoint("latency")]
#[measure_counter("conn")]
pub struct StreamWriteBlocked {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// Indicates that the write was the final offset of the stream
    is_fin: bool,

    /// The amount of time it took to process the write request
    ///
    /// Note that this includes both any syscall and encryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:write_errored")]
#[checkpoint("latency")]
pub struct StreamWriteErrored {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// Indicates that the write was the final offset of the stream
    is_fin: bool,

    /// The amount of time it took to process the write request
    ///
    /// Note that this includes both any syscall and encryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,

    /// The system `errno` from the returned error
    errno: Option<i32>,
}

#[event("stream:write_key_updated")]
pub struct StreamWriteKeyUpdated {
    key_phase: u8,
}

#[event("stream:write_allocated")]
#[measure_counter("conn")]
pub struct StreamWriteAllocated {
    /// The number of bytes that we allocated.
    #[measure("allocated_len", Bytes)]
    #[measure_counter("allocated_len.conn", Bytes)]
    allocated_len: usize,
}

#[event("stream:write_shutdown")]
#[checkpoint("latency")]
pub struct StreamWriteShutdown {
    /// The number of bytes in the send buffer at the time of shutdown
    #[measure("buffer_len", Bytes)]
    buffer_len: usize,

    /// If the stream required a background task to drive the stream shutdown
    #[bool_counter("background")]
    background: bool,
}

#[event("stream:write_socket_flushed")]
#[measure_counter("conn")]
pub struct StreamWriteSocketFlushed {
    /// The number of bytes that the stream tried to write to the socket
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    #[measure_counter("committed.conn", Bytes)]
    committed_len: usize,
}

#[event("stream:write_socket_blocked")]
#[measure_counter("conn")]
pub struct StreamWriteSocketBlocked {
    /// The number of bytes that the stream tried to write to the socket
    #[measure("provided", Bytes)]
    provided_len: usize,
}

#[event("stream:write_socket_errored")]
pub struct StreamWriteSocketErrored {
    /// The number of bytes that the stream tried to write to the socket
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The system `errno` from the returned error
    errno: Option<i32>,
}

#[event("stream:read_flushed")]
#[checkpoint("latency")]
#[measure_counter("conn")]
pub struct StreamReadFlushed {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount that was read into the provided buffer
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    #[measure_counter("committed.conn", Bytes)]
    committed_len: usize,

    /// The amount of time it took to process the read request
    ///
    /// Note that this includes both any syscall and decryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:read_fin_flushed")]
#[checkpoint("latency")]
#[measure_counter("conn")]
pub struct StreamReadFinFlushed {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount of time it took to process the read request
    ///
    /// Note that this includes both any syscall and decryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:read_blocked")]
#[checkpoint("latency")]
pub struct StreamReadBlocked {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount of time it took to process the read request
    ///
    /// Note that this includes both any syscall and decryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:read_errored")]
#[checkpoint("latency")]
pub struct StreamReadErrored {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount of time it took to process the read request
    ///
    /// Note that this includes both any syscall and decryption overhead
    #[measure("processing_duration", Duration)]
    #[measure_counter("processing_duration.conn", Duration)]
    processing_duration: core::time::Duration,

    /// The system `errno` from the returned error
    errno: Option<i32>,
}

#[event("stream:read_key_updated")]
pub struct StreamReadKeyUpdated {
    key_phase: u8,
}

#[event("stream:read_shutdown")]
#[checkpoint("latency")]
pub struct StreamReadShutdown {
    /// If the stream required a background task to drive the stream shutdown
    #[bool_counter("background")]
    background: bool,
}

#[event("stream:read_socket_flushed")]
#[measure_counter("conn")]
pub struct StreamReadSocketFlushed {
    /// The number of bytes that the stream tried to read from the socket
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount that was read into the provided buffer
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    #[measure_counter("committed.conn", Bytes)]
    committed_len: usize,
}

#[event("stream:read_socket_blocked")]
#[measure_counter("conn")]
pub struct StreamReadSocketBlocked {
    /// The number of bytes that the stream tried to read from the socket
    #[measure("capacity", Bytes)]
    capacity: usize,
}

#[event("stream:read_socket_errored")]
pub struct StreamReadSocketErrored {
    /// The number of bytes that the stream tried to read from the socket
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The system `errno` from the returned error
    errno: Option<i32>,
}

#[event("stream:decrypt_packet")]
pub struct StreamDecryptPacket {
    /// Did we decrypt the packet in place, or were we able to merge the copy and decrypt?
    #[bool_counter("decrypted_in_place")]
    decrypted_in_place: bool,

    /// The number of bytes we were forced to copy after decrypting in the packet buffer.
    ///
    /// This means that the application buffer was insufficiently large to allow us to directly
    /// copy as part of the decrypt. This can be non-zero even with decrypted_in_place=false, if we
    /// decrypted into the reassembly buffer. Right now it doesn't take into account zero-copy
    /// reads from the reassembly buffer (e.g., with specialized Bytes).
    #[measure("forced_copy", Bytes)]
    forced_copy: usize,

    /// The application buffer size that would avoid copies.
    #[measure("required_application_buffer", Bytes)]
    required_application_buffer: usize,
}

/// Tracks stream connect where dcQUIC owns the TCP connect().
#[event("stream:tcp_connect")]
#[subject(endpoint)]
pub struct StreamTcpConnect {
    #[bool_counter("error")]
    error: bool,

    // This includes the error latencies.
    //
    // FIXME: Support Option<Duration> in metrics to make it much easier to record timers
    // optionally.
    #[timer("tcp_latency")]
    latency: core::time::Duration,
}

/// Tracks stream connect where dcQUIC owns the TCP connect().
#[event("stream:connect")]
#[subject(endpoint)]
pub struct StreamConnect {
    #[bool_counter("error")]
    error: bool,

    #[nominal_counter("tcp")]
    tcp_success: MaybeBoolCounter,

    #[nominal_counter("handshake")]
    handshake_success: MaybeBoolCounter,
}

/// Used for cases where we are racing multiple futures and exit if any of them fail, and so
/// recording success is not just a boolean value.
enum MaybeBoolCounter {
    Success,
    Failure,
    Aborted,
}

/// Tracks stream connect errors.
///
/// Currently only emitted in cases where dcQUIC owns the TCP connect too.
#[event("stream:connect_error")]
#[subject(endpoint)]
pub struct StreamConnectError {
    #[nominal_counter("reason")]
    reason: StreamTcpConnectErrorReason,
}

/// Note that there's no guarantee of a particular reason if multiple reasons ~simultaneously
/// terminate the connection.
pub enum StreamTcpConnectErrorReason {
    /// TCP connect failed.
    TcpConnect,

    /// Handshake failed to produce credentials.
    Handshake,

    /// When the connect future is dropped prior to returning any result.
    ///
    /// Usually indicates a timeout in the application.
    Aborted,
}

#[event("stream:packet_transmitted")]
pub struct StreamPacketTransmitted {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the application data in the packet
    #[measure("payload_len", Bytes)]
    #[counter("payload_len.total", Bytes)]
    #[measure_counter("payload_len.conn", Bytes)]
    payload_len: usize,

    /// The packet number of the transmitted packet
    packet_number: u64,

    /// The offset in the stream of the first byte in the packet
    stream_offset: u64,

    /// Whether the packet contained the final bytes of the stream
    is_fin: bool,

    #[bool_counter("retransmission")]
    is_retransmission: bool,
}

#[event("stream:probe_transmitted")]
pub struct StreamProbeTransmitted {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The packet number of the transmitted packet
    packet_number: u64,
}

#[event("stream:packet_received")]
pub struct StreamPacketReceived {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the application data in the packet
    #[measure("payload_len", Bytes)]
    #[counter("payload_len.total", Bytes)]
    #[measure_counter("payload_len.conn", Bytes)]
    payload_len: usize,

    /// The packet number of the received packet
    packet_number: u64,

    /// The offset in the stream of the first byte in the packet
    stream_offset: u64,

    /// Whether the packet contained the final bytes of the stream
    is_fin: bool,

    #[bool_counter("retransmission")]
    is_retransmission: bool,
}

/// Indicates that a packet was lost on a stream
#[event("stream:packet_lost")]
pub struct StreamPacketLost {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the application data in the packet
    #[measure("payload_len", Bytes)]
    #[counter("payload_len.total", Bytes)]
    #[measure_counter("payload_len.conn", Bytes)]
    payload_len: usize,

    /// The packet number of the lost packet
    packet_number: u64,

    /// The offset in the stream of the first byte in the packet
    stream_offset: u64,

    /// The time the packet was originally sent
    time_sent: Timestamp,

    /// The amount of time between when the packet was sent and when it was detected as lost
    #[measure("lifetime", Duration)]
    lifetime: core::time::Duration,

    #[bool_counter("retransmission")]
    is_retransmission: bool,
}

/// Indicates that a packet was acknowledged on a stream
#[event("stream:packet_acked")]
pub struct StreamPacketAcked {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the application data in the packet
    #[measure("payload_len", Bytes)]
    #[counter("payload_len.total", Bytes)]
    #[measure_counter("payload_len.conn", Bytes)]
    payload_len: usize,

    /// The packet number of the acknowledged packet
    packet_number: u64,

    /// The offset in the stream of the first byte in the packet
    stream_offset: u64,

    /// The time the packet was originally sent
    time_sent: Timestamp,

    /// The amount of time between when the packet was sent and when it was detected as lost
    #[measure("lifetime", Duration)]
    lifetime: core::time::Duration,

    #[bool_counter("retransmission")]
    is_retransmission: bool,
}

/// Indicates that a packet was retransmitted on a stream but was not actually lost
#[event("stream:packet_spuriously_retransmitted")]
pub struct StreamPacketSpuriouslyRetransmitted {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the application data in the packet
    #[measure("payload_len", Bytes)]
    #[counter("payload_len.total", Bytes)]
    #[measure_counter("payload_len.conn", Bytes)]
    payload_len: usize,

    /// The packet number of the packet
    packet_number: u64,

    /// The offset in the stream of the first byte in the packet
    stream_offset: u64,

    /// Whether the packet contained the final bytes of the stream
    is_fin: bool,

    #[bool_counter("retransmission")]
    is_retransmission: bool,
}

/// Indicates that the stream received additional flow control credits
#[event("stream:max_data_received")]
pub struct StreamMaxDataReceived {
    /// The number of bytes of flow control credits received
    #[measure("increase", Bytes)]
    #[counter("increase.total", Bytes)]
    increase: u64,

    /// The new offset of the stream
    new_max_data: u64,
}

#[event("stream:control_packet_transmitted")]
pub struct StreamControlPacketTransmitted {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the control data in the packet
    #[measure("control_data_len", Bytes)]
    control_data_len: usize,

    /// The packet number of the received control packet
    packet_number: u64,
}

#[event("stream:control_packet_received")]
pub struct StreamControlPacketReceived {
    /// The total size of the packet
    #[measure("packet_len", Bytes)]
    packet_len: usize,

    /// The size of the control data in the packet
    #[measure("control_data_len", Bytes)]
    control_data_len: usize,

    /// The packet number of the received control packet
    packet_number: u64,

    /// Whether the packet was successfully authenticated
    #[bool_counter("authenticated")]
    is_authenticated: bool,
}

#[event("stream:receiver_errored")]
pub struct StreamReceiverErrored {
    #[builder(crate::stream::recv::Error)]
    error: crate::stream::recv::Error,

    /// The location where the error originated
    source: s2n_quic_core::endpoint::Location,
}

#[event("stream:sender_errored")]
pub struct StreamSenderErrored {
    #[builder(crate::stream::send::Error)]
    error: crate::stream::send::Error,

    /// The location where the error originated
    source: s2n_quic_core::endpoint::Location,
}

// NOTE - This event MUST come last, since connection-level aggregation depends on it
#[event("connection:closed")]
// #[checkpoint("latency")]
pub struct ConnectionClosed {}

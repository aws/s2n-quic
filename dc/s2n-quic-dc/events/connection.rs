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

// NOTE - This event MUST come last, since connection-level aggregation depends on it
#[event("connection:closed")]
// #[checkpoint("latency")]
pub struct ConnectionClosed {}

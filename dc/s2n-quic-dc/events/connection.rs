// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("stream:write_flushed")]
#[checkpoint("latency")]
pub struct StreamWriteFlushed {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    committed_len: usize,

    /// The amount of time it took to process the write request
    ///
    /// Note that this includes both any syscall and encryption overhead
    #[measure("processing_duration", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:write_fin_flushed")]
#[checkpoint("latency")]
pub struct StreamWriteFinFlushed {
    /// The number of bytes that the application tried to write
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    committed_len: usize,

    /// The amount of time it took to process the write request
    ///
    /// Note that this includes both any syscall and encryption overhead
    #[measure("processing_duration", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:write_blocked")]
#[checkpoint("latency")]
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
    processing_duration: core::time::Duration,

    /// The system `errno` from the returned error
    errno: Option<i32>,
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
pub struct StreamWriteSocketFlushed {
    /// The number of bytes that the stream tried to write to the socket
    #[measure("provided", Bytes)]
    provided_len: usize,

    /// The amount that was written
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    committed_len: usize,
}

#[event("stream:write_socket_blocked")]
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
pub struct StreamReadFlushed {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount that was read into the provided buffer
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    committed_len: usize,

    /// The amount of time it took to process the read request
    ///
    /// Note that this includes both any syscall and decryption overhead
    #[measure("processing_duration", Duration)]
    processing_duration: core::time::Duration,
}

#[event("stream:read_fin_flushed")]
#[checkpoint("latency")]
pub struct StreamReadFinFlushed {
    /// The number of bytes that the application tried to read
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount of time it took to process the read request
    ///
    /// Note that this includes both any syscall and decryption overhead
    #[measure("processing_duration", Duration)]
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
    processing_duration: core::time::Duration,

    /// The system `errno` from the returned error
    errno: Option<i32>,
}

#[event("stream:read_shutdown")]
#[checkpoint("latency")]
pub struct StreamReadShutdown {
    /// If the stream required a background task to drive the stream shutdown
    #[bool_counter("background")]
    background: bool,
}

#[event("stream:read_socket_flushed")]
pub struct StreamReadSocketFlushed {
    /// The number of bytes that the stream tried to read from the socket
    #[measure("capacity", Bytes)]
    capacity: usize,

    /// The amount that was read into the provided buffer
    #[measure("committed", Bytes)]
    #[counter("committed.total", Bytes)]
    committed_len: usize,
}

#[event("stream:read_socket_blocked")]
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

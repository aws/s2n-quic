// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Emitted when a TCP acceptor is started
#[event("acceptor:tcp:started")]
#[subject(endpoint)]
struct AcceptorTcpStarted<'a> {
    /// The id of the acceptor worker
    id: usize,

    /// The local address of the acceptor
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    local_address: SocketAddress<'a>,

    /// The backlog size
    backlog: usize,
}

/// Emitted when a TCP acceptor completes a single iteration of the event loop
#[event("acceptor:tcp:loop_iteration_completed")]
#[subject(endpoint)]
struct AcceptorTcpLoopIterationCompleted {
    /// The number of streams that are waiting on initial packets
    #[measure("pending_streams")]
    pending_streams: usize,

    /// The number of slots that are not currently processing a stream
    #[measure("slots_idle")]
    slots_idle: usize,

    /// The percentage of slots currently processing streams
    #[measure("slot_utilization", Percent)]
    slot_utilization: f32,

    /// The amount of time it took to complete the iteration
    #[timer("processing_duration")]
    processing_duration: core::time::Duration,

    /// The computed max sojourn time that is allowed for streams
    ///
    /// If streams consume more time than this value to initialize, they
    /// may potentially be replaced by more recent streams.
    #[measure("max_sojourn_time", Duration)]
    max_sojourn_time: core::time::Duration,
}

/// Emitted when a fresh TCP stream is enqueued for processing
#[event("acceptor:tcp:fresh:enqueued")]
#[subject(endpoint)]
struct AcceptorTcpFreshEnqueued<'a> {
    /// The remote address of the TCP stream
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,
}

/// Emitted when a the TCP acceptor has completed a batch of stream enqueues
#[event("acceptor:tcp:fresh:batch_completed")]
#[subject(endpoint)]
struct AcceptorTcpFreshBatchCompleted {
    /// The number of fresh TCP streams enqueued in this batch
    #[measure("enqueued")]
    enqueued: usize,

    /// The number of fresh TCP streams dropped in this batch due to capacity limits
    #[measure("dropped")]
    dropped: usize,

    /// The number of TCP streams that errored in this batch
    #[measure("errored")]
    errored: usize,
}

/// Emitted when a TCP stream has been dropped
#[event("acceptor:tcp:stream_dropped")]
#[subject(endpoint)]
struct AcceptorTcpStreamDropped<'a> {
    /// The remote address of the TCP stream
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    #[nominal_counter("reason")]
    reason: AcceptorTcpStreamDropReason,
}

enum AcceptorTcpStreamDropReason {
    /// There were more streams in the TCP backlog than the userspace queue can store
    FreshQueueAtCapacity,

    /// There are no available slots for processing
    SlotsAtCapacity,
}

/// Emitted when a TCP stream has been replaced by another stream
#[event("acceptor:tcp:stream_replaced")]
#[subject(endpoint)]
struct AcceptorTcpStreamReplaced<'a> {
    /// The remote address of the stream being replaced
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The amount of time that the stream spent in the accept queue before
    /// being replaced with another
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,

    /// The amount of bytes buffered on the stream
    #[measure("buffer_len", Bytes)]
    buffer_len: usize,
}

/// Emitted when a full packet has been received on the TCP stream
#[event("acceptor:tcp:packet_received")]
#[subject(endpoint)]
struct AcceptorTcpPacketReceived<'a> {
    /// The address of the packet's sender
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The credential ID of the packet
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The stream ID of the packet
    stream_id: u64,

    /// The payload length of the packet
    #[measure("payload_len", Bytes)]
    payload_len: usize,

    /// If the packet includes the final bytes of the stream
    #[bool_counter("is_fin")]
    is_fin: bool,

    /// If the packet includes the final offset of the stream
    #[bool_counter("is_fin_known")]
    is_fin_known: bool,

    /// The amount of time the TCP stream spent in the queue before receiving
    /// the initial packet
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,
}

/// Emitted when the TCP acceptor received an invalid initial packet
#[event("acceptor:tcp:packet_dropped")]
#[subject(endpoint)]
struct AcceptorTcpPacketDropped<'a> {
    /// The address of the packet's sender
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The reason the packet was dropped
    #[nominal_counter("reason")]
    reason: AcceptorPacketDropReason,

    /// The amount of time the TCP stream spent in the queue before receiving
    /// an error
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,
}

/// Emitted when the TCP stream has been enqueued for the application
#[event("acceptor:tcp:stream_enqueued")]
#[subject(endpoint)]
struct AcceptorTcpStreamEnqueued<'a> {
    /// The address of the stream's peer
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The credential ID of the stream
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The ID of the stream
    stream_id: u64,

    /// The amount of time the TCP stream spent in the queue before being enqueued
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,

    /// The number of times the stream was blocked on receiving more data
    #[measure("blocked_count")]
    blocked_count: usize,
}

/// Emitted when the TCP acceptor encounters an IO error
#[event("acceptor:tcp:io_error")]
#[subject(endpoint)]
struct AcceptorTcpIoError<'a> {
    /// The error encountered
    #[builder(&'a std::io::Error)]
    error: &'a std::io::Error,
}

/// Emitted when the TCP stream has been sent over a Unix domain socket
#[event("acceptor:tcp:socket_sent")]
#[subject(endpoint)]
struct AcceptorTcpSocketSent<'a>   {
    /// The credential ID of the stream
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The ID of the stream
    stream_id: u64,

    /// The amount of time the TCP stream spent in the queue before being sent over Unix domain socket
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,

    /// The number of times the Unix domain socket was blocked on send
    #[counter("blocked_count_host")]
    #[measure("blocked_count_stream")]
    blocked_count: usize,

    /// The len of the payload sent over the Unix domain socket
    #[measure("len", Bytes)]
    payload_len: usize,
}

/// Emitted when a TCP stream has been received from a Unix domain socket
#[event("acceptor:tcp:socket_received")]
#[subject(endpoint)]
struct AcceptorTcpSocketReceived<'a>   {
    /// The address of the stream's peer
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,
    
    /// The credential ID of the stream
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The ID of the stream
    stream_id: u64,

    /// The amount of time taken from socket send to socket receive, including waiting if the kernel queue is full
    #[timer("transfer_time")]
    transfer_time: core::time::Duration,

    /// The len of the payload sent over the Unix domain socket
    #[measure("len", Bytes)]
    payload_len: usize,
}

/// Emitted when a UDP acceptor is started
#[event("acceptor:udp:started")]
#[subject(endpoint)]
struct AcceptorUdpStarted<'a> {
    /// The id of the acceptor worker
    id: usize,

    /// The local address of the acceptor
    local_address: SocketAddress<'a>,
}

/// Emitted when a UDP datagram is received by the acceptor
#[event("acceptor:udp:datagram_received")]
#[subject(endpoint)]
struct AcceptorUdpDatagramReceived<'a> {
    /// The address of the datagram's sender
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The len of the datagram
    #[measure("len", Bytes)]
    len: usize,
}

/// Emitted when the UDP acceptor parsed a packet contained in a datagram
#[event("acceptor:udp:packet_received")]
#[subject(endpoint)]
struct AcceptorUdpPacketReceived<'a> {
    /// The address of the packet's sender
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The credential ID of the packet
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The stream ID of the packet
    stream_id: u64,

    /// The payload length of the packet
    #[measure("payload_len", Bytes)]
    payload_len: usize,

    /// If the packets is a zero offset in the stream
    #[bool_counter("is_zero_offset")]
    is_zero_offset: bool,

    /// If the packet is a retransmission
    #[bool_counter("is_retransmission")]
    is_retransmission: bool,

    /// If the packet includes the final bytes of the stream
    #[bool_counter("is_fin")]
    is_fin: bool,

    /// If the packet includes the final offset of the stream
    #[bool_counter("is_fin_known")]
    is_fin_known: bool,
}

/// Emitted when the UDP acceptor received an invalid initial packet
#[event("acceptor:udp:packet_dropped")]
#[subject(endpoint)]
struct AcceptorUdpPacketDropped<'a> {
    /// The address of the packet's sender
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The reason the packet was dropped
    #[nominal_counter("reason")]
    reason: AcceptorPacketDropReason,
}

/// Emitted when the UDP stream has been enqueued for the application
#[event("acceptor:udp:stream_enqueued")]
#[subject(endpoint)]
struct AcceptorUdpStreamEnqueued<'a> {
    /// The address of the stream's peer
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The credential ID of the stream
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The ID of the stream
    stream_id: u64,
}

/// Emitted when the UDP acceptor encounters an IO error
#[event("acceptor:udp:io_error")]
#[subject(endpoint)]
struct AcceptorUdpIoError<'a> {
    /// The error encountered
    #[builder(&'a std::io::Error)]
    error: &'a std::io::Error,
}

/// Emitted when a stream has been pruned
#[event("acceptor:stream_pruned")]
#[subject(endpoint)]
struct AcceptorStreamPruned<'a> {
    /// The remote address of the stream
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The credential ID of the stream
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The ID of the stream
    stream_id: u64,

    /// The amount of time that the stream spent in the accept queue before
    /// being pruned
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,

    #[nominal_counter("reason")]
    reason: AcceptorStreamPruneReason,
}

enum AcceptorStreamPruneReason {
    MaxSojournTimeExceeded,
    AcceptQueueCapacityExceeded,
}

/// Emitted when a stream has been dequeued by the application
#[event("acceptor:stream_dequeued")]
#[subject(endpoint)]
struct AcceptorStreamDequeued<'a> {
    /// The remote address of the stream
    #[builder(&'a s2n_quic_core::inet::SocketAddress)]
    remote_address: SocketAddress<'a>,

    /// The credential ID of the stream
    #[snapshot("[HIDDEN]")]
    credential_id: &'a [u8],

    /// The ID of the stream
    stream_id: u64,

    /// The amount of time that the stream spent in the accept queue before
    /// being dequeued
    #[timer("sojourn_time")]
    sojourn_time: core::time::Duration,
}

enum AcceptorPacketDropReason {
    UnexpectedEof,
    UnexpectedBytes,
    LengthCapacityExceeded,
    InvariantViolation { message: &'static str },
}

impl IntoEvent<builder::AcceptorPacketDropReason> for s2n_codec::DecoderError {
    fn into_event(self) -> builder::AcceptorPacketDropReason {
        use builder::AcceptorPacketDropReason as Reason;
        use s2n_codec::DecoderError;
        match self {
            DecoderError::UnexpectedEof(_) => Reason::UnexpectedEof {},
            DecoderError::UnexpectedBytes(_) => Reason::UnexpectedBytes {},
            DecoderError::LengthCapacityExceeded => Reason::LengthCapacityExceeded {},
            DecoderError::InvariantViolation(message) => Reason::InvariantViolation { message },
        }
    }
}

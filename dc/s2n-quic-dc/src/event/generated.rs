// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

#![allow(clippy::needless_lifetimes)]
use super::*;
pub(crate) mod metrics;
pub mod api {
    #![doc = r" This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)"]
    use super::*;
    #[allow(unused_imports)]
    use crate::event::metrics::aggregate;
    pub use s2n_quic_core::event::api::{EndpointType, SocketAddress, Subject};
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a TCP acceptor is started"]
    pub struct AcceptorTcpStarted<'a> {
        #[doc = " The id of the acceptor worker"]
        pub id: usize,
        #[doc = " The local address of the acceptor"]
        pub local_address: SocketAddress<'a>,
        #[doc = " The backlog size"]
        pub backlog: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpStarted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpStarted");
            fmt.field("id", &self.id);
            fmt.field("local_address", &self.local_address);
            fmt.field("backlog", &self.backlog);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpStarted<'a> {
        const NAME: &'static str = "acceptor:tcp:started";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a TCP acceptor completes a single iteration of the event loop"]
    pub struct AcceptorTcpLoopIterationCompleted {
        #[doc = " The number of streams that are waiting on initial packets"]
        pub pending_streams: usize,
        #[doc = " The number of slots that are not currently processing a stream"]
        pub slots_idle: usize,
        #[doc = " The percentage of slots currently processing streams"]
        pub slot_utilization: f32,
        #[doc = " The amount of time it took to complete the iteration"]
        pub processing_duration: core::time::Duration,
        #[doc = " The computed max sojourn time that is allowed for streams"]
        #[doc = ""]
        #[doc = " If streams consume more time than this value to initialize, they"]
        #[doc = " may potentially be replaced by more recent streams."]
        pub max_sojourn_time: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for AcceptorTcpLoopIterationCompleted {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpLoopIterationCompleted");
            fmt.field("pending_streams", &self.pending_streams);
            fmt.field("slots_idle", &self.slots_idle);
            fmt.field("slot_utilization", &self.slot_utilization);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.field("max_sojourn_time", &self.max_sojourn_time);
            fmt.finish()
        }
    }
    impl Event for AcceptorTcpLoopIterationCompleted {
        const NAME: &'static str = "acceptor:tcp:loop_iteration_completed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a fresh TCP stream is enqueued for processing"]
    pub struct AcceptorTcpFreshEnqueued<'a> {
        #[doc = " The remote address of the TCP stream"]
        pub remote_address: SocketAddress<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpFreshEnqueued<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpFreshEnqueued");
            fmt.field("remote_address", &self.remote_address);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpFreshEnqueued<'a> {
        const NAME: &'static str = "acceptor:tcp:fresh:enqueued";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a the TCP acceptor has completed a batch of stream enqueues"]
    pub struct AcceptorTcpFreshBatchCompleted {
        #[doc = " The number of fresh TCP streams enqueued in this batch"]
        pub enqueued: usize,
        #[doc = " The number of fresh TCP streams dropped in this batch due to capacity limits"]
        pub dropped: usize,
        #[doc = " The number of TCP streams that errored in this batch"]
        pub errored: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for AcceptorTcpFreshBatchCompleted {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpFreshBatchCompleted");
            fmt.field("enqueued", &self.enqueued);
            fmt.field("dropped", &self.dropped);
            fmt.field("errored", &self.errored);
            fmt.finish()
        }
    }
    impl Event for AcceptorTcpFreshBatchCompleted {
        const NAME: &'static str = "acceptor:tcp:fresh:batch_completed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a TCP stream has been dropped"]
    pub struct AcceptorTcpStreamDropped<'a> {
        #[doc = " The remote address of the TCP stream"]
        pub remote_address: SocketAddress<'a>,
        pub reason: AcceptorTcpStreamDropReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpStreamDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpStreamDropped");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpStreamDropped<'a> {
        const NAME: &'static str = "acceptor:tcp:stream_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a TCP stream has been replaced by another stream"]
    pub struct AcceptorTcpStreamReplaced<'a> {
        #[doc = " The remote address of the stream being replaced"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The amount of time that the stream spent in the accept queue before"]
        #[doc = " being replaced with another"]
        pub sojourn_time: core::time::Duration,
        #[doc = " The amount of bytes buffered on the stream"]
        pub buffer_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpStreamReplaced<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpStreamReplaced");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("sojourn_time", &self.sojourn_time);
            fmt.field("buffer_len", &self.buffer_len);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpStreamReplaced<'a> {
        const NAME: &'static str = "acceptor:tcp:stream_replaced";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a full packet has been received on the TCP stream"]
    pub struct AcceptorTcpPacketReceived<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The credential ID of the packet"]
        pub credential_id: &'a [u8],
        #[doc = " The stream ID of the packet"]
        pub stream_id: u64,
        #[doc = " The payload length of the packet"]
        pub payload_len: usize,
        #[doc = " If the packet includes the final bytes of the stream"]
        pub is_fin: bool,
        #[doc = " If the packet includes the final offset of the stream"]
        pub is_fin_known: bool,
        #[doc = " The amount of time the TCP stream spent in the queue before receiving"]
        #[doc = " the initial packet"]
        pub sojourn_time: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpPacketReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpPacketReceived");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("stream_id", &self.stream_id);
            fmt.field("payload_len", &self.payload_len);
            fmt.field("is_fin", &self.is_fin);
            fmt.field("is_fin_known", &self.is_fin_known);
            fmt.field("sojourn_time", &self.sojourn_time);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpPacketReceived<'a> {
        const NAME: &'static str = "acceptor:tcp:packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the TCP acceptor received an invalid initial packet"]
    pub struct AcceptorTcpPacketDropped<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The reason the packet was dropped"]
        pub reason: AcceptorPacketDropReason,
        #[doc = " The amount of time the TCP stream spent in the queue before receiving"]
        #[doc = " an error"]
        pub sojourn_time: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpPacketDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpPacketDropped");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("reason", &self.reason);
            fmt.field("sojourn_time", &self.sojourn_time);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpPacketDropped<'a> {
        const NAME: &'static str = "acceptor:tcp:packet_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the TCP stream has been enqueued for the application"]
    pub struct AcceptorTcpStreamEnqueued<'a> {
        #[doc = " The address of the stream's peer"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
        #[doc = " The amount of time the TCP stream spent in the queue before being enqueued"]
        pub sojourn_time: core::time::Duration,
        #[doc = " The number of times the stream was blocked on receiving more data"]
        pub blocked_count: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpStreamEnqueued<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpStreamEnqueued");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("stream_id", &self.stream_id);
            fmt.field("sojourn_time", &self.sojourn_time);
            fmt.field("blocked_count", &self.blocked_count);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpStreamEnqueued<'a> {
        const NAME: &'static str = "acceptor:tcp:stream_enqueued";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the TCP acceptor encounters an IO error"]
    pub struct AcceptorTcpIoError<'a> {
        #[doc = " The error encountered"]
        pub error: &'a std::io::Error,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorTcpIoError<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorTcpIoError");
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorTcpIoError<'a> {
        const NAME: &'static str = "acceptor:tcp:io_error";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a UDP acceptor is started"]
    pub struct AcceptorUdpStarted<'a> {
        #[doc = " The id of the acceptor worker"]
        pub id: usize,
        #[doc = " The local address of the acceptor"]
        pub local_address: SocketAddress<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorUdpStarted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorUdpStarted");
            fmt.field("id", &self.id);
            fmt.field("local_address", &self.local_address);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorUdpStarted<'a> {
        const NAME: &'static str = "acceptor:udp:started";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a UDP datagram is received by the acceptor"]
    pub struct AcceptorUdpDatagramReceived<'a> {
        #[doc = " The address of the datagram's sender"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The len of the datagram"]
        pub len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorUdpDatagramReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorUdpDatagramReceived");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("len", &self.len);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorUdpDatagramReceived<'a> {
        const NAME: &'static str = "acceptor:udp:datagram_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the UDP acceptor parsed a packet contained in a datagram"]
    pub struct AcceptorUdpPacketReceived<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The credential ID of the packet"]
        pub credential_id: &'a [u8],
        #[doc = " The stream ID of the packet"]
        pub stream_id: u64,
        #[doc = " The payload length of the packet"]
        pub payload_len: usize,
        #[doc = " If the packets is a zero offset in the stream"]
        pub is_zero_offset: bool,
        #[doc = " If the packet is a retransmission"]
        pub is_retransmission: bool,
        #[doc = " If the packet includes the final bytes of the stream"]
        pub is_fin: bool,
        #[doc = " If the packet includes the final offset of the stream"]
        pub is_fin_known: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorUdpPacketReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorUdpPacketReceived");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("stream_id", &self.stream_id);
            fmt.field("payload_len", &self.payload_len);
            fmt.field("is_zero_offset", &self.is_zero_offset);
            fmt.field("is_retransmission", &self.is_retransmission);
            fmt.field("is_fin", &self.is_fin);
            fmt.field("is_fin_known", &self.is_fin_known);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorUdpPacketReceived<'a> {
        const NAME: &'static str = "acceptor:udp:packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the UDP acceptor received an invalid initial packet"]
    pub struct AcceptorUdpPacketDropped<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The reason the packet was dropped"]
        pub reason: AcceptorPacketDropReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorUdpPacketDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorUdpPacketDropped");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorUdpPacketDropped<'a> {
        const NAME: &'static str = "acceptor:udp:packet_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the UDP stream has been enqueued for the application"]
    pub struct AcceptorUdpStreamEnqueued<'a> {
        #[doc = " The address of the stream's peer"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorUdpStreamEnqueued<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorUdpStreamEnqueued");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("stream_id", &self.stream_id);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorUdpStreamEnqueued<'a> {
        const NAME: &'static str = "acceptor:udp:stream_enqueued";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the UDP acceptor encounters an IO error"]
    pub struct AcceptorUdpIoError<'a> {
        #[doc = " The error encountered"]
        pub error: &'a std::io::Error,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorUdpIoError<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorUdpIoError");
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorUdpIoError<'a> {
        const NAME: &'static str = "acceptor:udp:io_error";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a stream has been pruned"]
    pub struct AcceptorStreamPruned<'a> {
        #[doc = " The remote address of the stream"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
        #[doc = " The amount of time that the stream spent in the accept queue before"]
        #[doc = " being pruned"]
        pub sojourn_time: core::time::Duration,
        pub reason: AcceptorStreamPruneReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorStreamPruned<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorStreamPruned");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("stream_id", &self.stream_id);
            fmt.field("sojourn_time", &self.sojourn_time);
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorStreamPruned<'a> {
        const NAME: &'static str = "acceptor:stream_pruned";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a stream has been dequeued by the application"]
    pub struct AcceptorStreamDequeued<'a> {
        #[doc = " The remote address of the stream"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
        #[doc = " The amount of time that the stream spent in the accept queue before"]
        #[doc = " being dequeued"]
        pub sojourn_time: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for AcceptorStreamDequeued<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("AcceptorStreamDequeued");
            fmt.field("remote_address", &self.remote_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("stream_id", &self.stream_id);
            fmt.field("sojourn_time", &self.sojourn_time);
            fmt.finish()
        }
    }
    impl<'a> Event for AcceptorStreamDequeued<'a> {
        const NAME: &'static str = "acceptor:stream_dequeued";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum AcceptorTcpStreamDropReason {
        #[non_exhaustive]
        #[doc = " There were more streams in the TCP backlog than the userspace queue can store"]
        FreshQueueAtCapacity {},
        #[non_exhaustive]
        #[doc = " There are no available slots for processing"]
        SlotsAtCapacity {},
    }
    impl aggregate::AsVariant for AcceptorTcpStreamDropReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("FRESH_QUEUE_AT_CAPACITY\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("SLOTS_AT_CAPACITY\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::FreshQueueAtCapacity { .. } => 0usize,
                Self::SlotsAtCapacity { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum AcceptorStreamPruneReason {
        #[non_exhaustive]
        MaxSojournTimeExceeded {},
        #[non_exhaustive]
        AcceptQueueCapacityExceeded {},
    }
    impl aggregate::AsVariant for AcceptorStreamPruneReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("MAX_SOJOURN_TIME_EXCEEDED\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ACCEPT_QUEUE_CAPACITY_EXCEEDED\0"),
                id: 1usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::MaxSojournTimeExceeded { .. } => 0usize,
                Self::AcceptQueueCapacityExceeded { .. } => 1usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub enum AcceptorPacketDropReason {
        #[non_exhaustive]
        UnexpectedEof {},
        #[non_exhaustive]
        UnexpectedBytes {},
        #[non_exhaustive]
        LengthCapacityExceeded {},
        #[non_exhaustive]
        InvariantViolation { message: &'static str },
    }
    impl aggregate::AsVariant for AcceptorPacketDropReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNEXPECTED_EOF\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("UNEXPECTED_BYTES\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("LENGTH_CAPACITY_EXCEEDED\0"),
                id: 2usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("INVARIANT_VIOLATION\0"),
                id: 3usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::UnexpectedEof { .. } => 0usize,
                Self::UnexpectedBytes { .. } => 1usize,
                Self::LengthCapacityExceeded { .. } => 2usize,
                Self::InvariantViolation { .. } => 3usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionMeta {
        pub id: u64,
        pub timestamp: Timestamp,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionMeta");
            fmt.field("id", &self.id);
            fmt.field("timestamp", &self.timestamp);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointMeta {
        pub timestamp: Timestamp,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointMeta");
            fmt.field("timestamp", &self.timestamp);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionInfo {}
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionInfo {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionInfo");
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteFlushed {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " The amount that was written"]
        pub committed_len: usize,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteFlushed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteFlushed");
            fmt.field("provided_len", &self.provided_len);
            fmt.field("committed_len", &self.committed_len);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for StreamWriteFlushed {
        const NAME: &'static str = "stream:write_flushed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteFinFlushed {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " The amount that was written"]
        pub committed_len: usize,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteFinFlushed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteFinFlushed");
            fmt.field("provided_len", &self.provided_len);
            fmt.field("committed_len", &self.committed_len);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for StreamWriteFinFlushed {
        const NAME: &'static str = "stream:write_fin_flushed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteBlocked {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " Indicates that the write was the final offset of the stream"]
        pub is_fin: bool,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteBlocked {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteBlocked");
            fmt.field("provided_len", &self.provided_len);
            fmt.field("is_fin", &self.is_fin);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for StreamWriteBlocked {
        const NAME: &'static str = "stream:write_blocked";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteErrored {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " Indicates that the write was the final offset of the stream"]
        pub is_fin: bool,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteErrored {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteErrored");
            fmt.field("provided_len", &self.provided_len);
            fmt.field("is_fin", &self.is_fin);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.field("errno", &self.errno);
            fmt.finish()
        }
    }
    impl Event for StreamWriteErrored {
        const NAME: &'static str = "stream:write_errored";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteKeyUpdated {
        pub key_phase: u8,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteKeyUpdated {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteKeyUpdated");
            fmt.field("key_phase", &self.key_phase);
            fmt.finish()
        }
    }
    impl Event for StreamWriteKeyUpdated {
        const NAME: &'static str = "stream:write_key_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteShutdown {
        #[doc = " The number of bytes in the send buffer at the time of shutdown"]
        pub buffer_len: usize,
        #[doc = " If the stream required a background task to drive the stream shutdown"]
        pub background: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteShutdown {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteShutdown");
            fmt.field("buffer_len", &self.buffer_len);
            fmt.field("background", &self.background);
            fmt.finish()
        }
    }
    impl Event for StreamWriteShutdown {
        const NAME: &'static str = "stream:write_shutdown";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteSocketFlushed {
        #[doc = " The number of bytes that the stream tried to write to the socket"]
        pub provided_len: usize,
        #[doc = " The amount that was written"]
        pub committed_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteSocketFlushed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteSocketFlushed");
            fmt.field("provided_len", &self.provided_len);
            fmt.field("committed_len", &self.committed_len);
            fmt.finish()
        }
    }
    impl Event for StreamWriteSocketFlushed {
        const NAME: &'static str = "stream:write_socket_flushed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteSocketBlocked {
        #[doc = " The number of bytes that the stream tried to write to the socket"]
        pub provided_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteSocketBlocked {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteSocketBlocked");
            fmt.field("provided_len", &self.provided_len);
            fmt.finish()
        }
    }
    impl Event for StreamWriteSocketBlocked {
        const NAME: &'static str = "stream:write_socket_blocked";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamWriteSocketErrored {
        #[doc = " The number of bytes that the stream tried to write to the socket"]
        pub provided_len: usize,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamWriteSocketErrored {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamWriteSocketErrored");
            fmt.field("provided_len", &self.provided_len);
            fmt.field("errno", &self.errno);
            fmt.finish()
        }
    }
    impl Event for StreamWriteSocketErrored {
        const NAME: &'static str = "stream:write_socket_errored";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadFlushed {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount that was read into the provided buffer"]
        pub committed_len: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadFlushed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadFlushed");
            fmt.field("capacity", &self.capacity);
            fmt.field("committed_len", &self.committed_len);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for StreamReadFlushed {
        const NAME: &'static str = "stream:read_flushed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadFinFlushed {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadFinFlushed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadFinFlushed");
            fmt.field("capacity", &self.capacity);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for StreamReadFinFlushed {
        const NAME: &'static str = "stream:read_fin_flushed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadBlocked {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadBlocked {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadBlocked");
            fmt.field("capacity", &self.capacity);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.finish()
        }
    }
    impl Event for StreamReadBlocked {
        const NAME: &'static str = "stream:read_blocked";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadErrored {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadErrored {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadErrored");
            fmt.field("capacity", &self.capacity);
            fmt.field("processing_duration", &self.processing_duration);
            fmt.field("errno", &self.errno);
            fmt.finish()
        }
    }
    impl Event for StreamReadErrored {
        const NAME: &'static str = "stream:read_errored";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadKeyUpdated {
        pub key_phase: u8,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadKeyUpdated {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadKeyUpdated");
            fmt.field("key_phase", &self.key_phase);
            fmt.finish()
        }
    }
    impl Event for StreamReadKeyUpdated {
        const NAME: &'static str = "stream:read_key_updated";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadShutdown {
        #[doc = " If the stream required a background task to drive the stream shutdown"]
        pub background: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadShutdown {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadShutdown");
            fmt.field("background", &self.background);
            fmt.finish()
        }
    }
    impl Event for StreamReadShutdown {
        const NAME: &'static str = "stream:read_shutdown";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadSocketFlushed {
        #[doc = " The number of bytes that the stream tried to read from the socket"]
        pub capacity: usize,
        #[doc = " The amount that was read into the provided buffer"]
        pub committed_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadSocketFlushed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadSocketFlushed");
            fmt.field("capacity", &self.capacity);
            fmt.field("committed_len", &self.committed_len);
            fmt.finish()
        }
    }
    impl Event for StreamReadSocketFlushed {
        const NAME: &'static str = "stream:read_socket_flushed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadSocketBlocked {
        #[doc = " The number of bytes that the stream tried to read from the socket"]
        pub capacity: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadSocketBlocked {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadSocketBlocked");
            fmt.field("capacity", &self.capacity);
            fmt.finish()
        }
    }
    impl Event for StreamReadSocketBlocked {
        const NAME: &'static str = "stream:read_socket_blocked";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct StreamReadSocketErrored {
        #[doc = " The number of bytes that the stream tried to read from the socket"]
        pub capacity: usize,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamReadSocketErrored {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamReadSocketErrored");
            fmt.field("capacity", &self.capacity);
            fmt.field("errno", &self.errno);
            fmt.finish()
        }
    }
    impl Event for StreamReadSocketErrored {
        const NAME: &'static str = "stream:read_socket_errored";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Tracks stream connect where dcQUIC owns the TCP connect()."]
    pub struct StreamTcpConnect {
        pub error: bool,
        pub latency: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamTcpConnect {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamTcpConnect");
            fmt.field("error", &self.error);
            fmt.field("latency", &self.latency);
            fmt.finish()
        }
    }
    impl Event for StreamTcpConnect {
        const NAME: &'static str = "stream:tcp_connect";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Tracks stream connect where dcQUIC owns the TCP connect()."]
    pub struct StreamConnect {
        pub error: bool,
        pub tcp_success: MaybeBoolCounter,
        pub handshake_success: MaybeBoolCounter,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamConnect {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamConnect");
            fmt.field("error", &self.error);
            fmt.field("tcp_success", &self.tcp_success);
            fmt.field("handshake_success", &self.handshake_success);
            fmt.finish()
        }
    }
    impl Event for StreamConnect {
        const NAME: &'static str = "stream:connect";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Tracks stream connect errors."]
    #[doc = ""]
    #[doc = " Currently only emitted in cases where dcQUIC owns the TCP connect too."]
    pub struct StreamConnectError {
        pub reason: StreamTcpConnectErrorReason,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for StreamConnectError {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StreamConnectError");
            fmt.field("reason", &self.reason);
            fmt.finish()
        }
    }
    impl Event for StreamConnectError {
        const NAME: &'static str = "stream:connect_error";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionClosed {}
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionClosed {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionClosed");
            fmt.finish()
        }
    }
    impl Event for ConnectionClosed {
        const NAME: &'static str = "connection:closed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Used for cases where we are racing multiple futures and exit if any of them fail, and so"]
    #[doc = " recording success is not just a boolean value."]
    pub enum MaybeBoolCounter {
        #[non_exhaustive]
        Success {},
        #[non_exhaustive]
        Failure {},
        #[non_exhaustive]
        Aborted {},
    }
    impl aggregate::AsVariant for MaybeBoolCounter {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("SUCCESS\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("FAILURE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ABORTED\0"),
                id: 2usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::Success { .. } => 0usize,
                Self::Failure { .. } => 1usize,
                Self::Aborted { .. } => 2usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Note that there's no guarantee of a particular reason if multiple reasons ~simultaneously"]
    #[doc = " terminate the connection."]
    pub enum StreamTcpConnectErrorReason {
        #[non_exhaustive]
        #[doc = " TCP connect failed."]
        TcpConnect {},
        #[non_exhaustive]
        #[doc = " Handshake failed to produce credentials."]
        Handshake {},
        #[non_exhaustive]
        #[doc = " When the connect future is dropped prior to returning any result."]
        #[doc = ""]
        #[doc = " Usually indicates a timeout in the application."]
        Aborted {},
    }
    impl aggregate::AsVariant for StreamTcpConnectErrorReason {
        const VARIANTS: &'static [aggregate::info::Variant] = &[
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("TCP_CONNECT\0"),
                id: 0usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("HANDSHAKE\0"),
                id: 1usize,
            }
            .build(),
            aggregate::info::variant::Builder {
                name: aggregate::info::Str::new("ABORTED\0"),
                id: 2usize,
            }
            .build(),
        ];
        #[inline]
        fn variant_idx(&self) -> usize {
            match self {
                Self::TcpConnect { .. } => 0usize,
                Self::Handshake { .. } => 1usize,
                Self::Aborted { .. } => 2usize,
            }
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointInitialized<'a> {
        pub acceptor_addr: SocketAddress<'a>,
        pub handshake_addr: SocketAddress<'a>,
        pub tcp: bool,
        pub udp: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointInitialized<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointInitialized");
            fmt.field("acceptor_addr", &self.acceptor_addr);
            fmt.field("handshake_addr", &self.handshake_addr);
            fmt.field("tcp", &self.tcp);
            fmt.field("udp", &self.udp);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointInitialized<'a> {
        const NAME: &'static str = "endpoint:initialized";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PathSecretMapInitialized {
        #[doc = " The capacity of the path secret map"]
        pub capacity: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PathSecretMapInitialized {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapInitialized");
            fmt.field("capacity", &self.capacity);
            fmt.finish()
        }
    }
    impl Event for PathSecretMapInitialized {
        const NAME: &'static str = "path_secret_map:initialized";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PathSecretMapUninitialized {
        #[doc = " The capacity of the path secret map"]
        pub capacity: usize,
        #[doc = " The number of entries in the map"]
        pub entries: usize,
        pub lifetime: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PathSecretMapUninitialized {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapUninitialized");
            fmt.field("capacity", &self.capacity);
            fmt.field("entries", &self.entries);
            fmt.field("lifetime", &self.lifetime);
            fmt.finish()
        }
    }
    impl Event for PathSecretMapUninitialized {
        const NAME: &'static str = "path_secret_map:uninitialized";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a background handshake is requested"]
    pub struct PathSecretMapBackgroundHandshakeRequested<'a> {
        pub peer_address: SocketAddress<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapBackgroundHandshakeRequested<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapBackgroundHandshakeRequested");
            fmt.field("peer_address", &self.peer_address);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapBackgroundHandshakeRequested<'a> {
        const NAME: &'static str = "path_secret_map:background_handshake_requested";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the entry is inserted into the path secret map"]
    pub struct PathSecretMapEntryInserted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapEntryInserted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapEntryInserted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapEntryInserted<'a> {
        const NAME: &'static str = "path_secret_map:entry_inserted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the entry is considered ready for use"]
    pub struct PathSecretMapEntryReady<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapEntryReady<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapEntryReady");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapEntryReady<'a> {
        const NAME: &'static str = "path_secret_map:entry_ready";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an entry is replaced by a new one for the same `peer_address`"]
    pub struct PathSecretMapEntryReplaced<'a> {
        pub peer_address: SocketAddress<'a>,
        pub new_credential_id: &'a [u8],
        pub previous_credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapEntryReplaced<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapEntryReplaced");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("new_credential_id", &"[HIDDEN]");
            fmt.field("previous_credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapEntryReplaced<'a> {
        const NAME: &'static str = "path_secret_map:entry_replaced";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an entry is evicted due to running out of space"]
    pub struct PathSecretMapIdEntryEvicted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
        #[doc = " Time since insertion of this entry"]
        pub age: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapIdEntryEvicted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapIdEntryEvicted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("age", &self.age);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapIdEntryEvicted<'a> {
        const NAME: &'static str = "path_secret_map:id_entry_evicted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an entry is evicted due to running out of space"]
    pub struct PathSecretMapAddressEntryEvicted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
        #[doc = " Time since insertion of this entry"]
        pub age: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapAddressEntryEvicted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapAddressEntryEvicted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("age", &self.age);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapAddressEntryEvicted<'a> {
        const NAME: &'static str = "path_secret_map:addr_entry_evicted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an UnknownPathSecret packet was sent"]
    pub struct UnknownPathSecretPacketSent<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for UnknownPathSecretPacketSent<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("UnknownPathSecretPacketSent");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for UnknownPathSecretPacketSent<'a> {
        const NAME: &'static str = "path_secret_map:unknown_path_secret_packet_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an UnknownPathSecret packet was received"]
    pub struct UnknownPathSecretPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for UnknownPathSecretPacketReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("UnknownPathSecretPacketReceived");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for UnknownPathSecretPacketReceived<'a> {
        const NAME: &'static str = "path_secret_map:unknown_path_secret_packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an UnknownPathSecret packet was authentic and processed"]
    pub struct UnknownPathSecretPacketAccepted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for UnknownPathSecretPacketAccepted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("UnknownPathSecretPacketAccepted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for UnknownPathSecretPacketAccepted<'a> {
        const NAME: &'static str = "path_secret_map:unknown_path_secret_packet_accepted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an UnknownPathSecret packet was rejected as invalid"]
    pub struct UnknownPathSecretPacketRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for UnknownPathSecretPacketRejected<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("UnknownPathSecretPacketRejected");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for UnknownPathSecretPacketRejected<'a> {
        const NAME: &'static str = "path_secret_map:unknown_path_secret_packet_rejected";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an UnknownPathSecret packet was dropped due to a missing entry"]
    pub struct UnknownPathSecretPacketDropped<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for UnknownPathSecretPacketDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("UnknownPathSecretPacketDropped");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for UnknownPathSecretPacketDropped<'a> {
        const NAME: &'static str = "path_secret_map:unknown_path_secret_packet_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when a credential is accepted (i.e., post packet authentication and passes replay"]
    #[doc = " check)."]
    pub struct KeyAccepted<'a> {
        pub credential_id: &'a [u8],
        pub key_id: u64,
        #[doc = " How far away this credential is from the leading edge of key IDs (after updating the edge)."]
        #[doc = ""]
        #[doc = " Zero if this shifted us forward."]
        pub gap: u64,
        #[doc = " How far away this credential is from the leading edge of key IDs (before updating the edge)."]
        #[doc = ""]
        #[doc = " Zero if this didn't change the leading edge."]
        pub forward_shift: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for KeyAccepted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("KeyAccepted");
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("key_id", &self.key_id);
            fmt.field("gap", &self.gap);
            fmt.field("forward_shift", &self.forward_shift);
            fmt.finish()
        }
    }
    impl<'a> Event for KeyAccepted<'a> {
        const NAME: &'static str = "path_secret_map:key_accepted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when credential replay was definitely detected"]
    pub struct ReplayDefinitelyDetected<'a> {
        pub credential_id: &'a [u8],
        pub key_id: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayDefinitelyDetected<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayDefinitelyDetected");
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("key_id", &self.key_id);
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayDefinitelyDetected<'a> {
        const NAME: &'static str = "path_secret_map:replay_definitely_detected";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when credential replay was potentially detected, but could not be verified"]
    #[doc = " due to a limiting tracking window"]
    pub struct ReplayPotentiallyDetected<'a> {
        pub credential_id: &'a [u8],
        pub key_id: u64,
        pub gap: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayPotentiallyDetected<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayPotentiallyDetected");
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("key_id", &self.key_id);
            fmt.field("gap", &self.gap);
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayPotentiallyDetected<'a> {
        const NAME: &'static str = "path_secret_map:replay_potentially_detected";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an ReplayDetected packet was sent"]
    pub struct ReplayDetectedPacketSent<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayDetectedPacketSent<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayDetectedPacketSent");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayDetectedPacketSent<'a> {
        const NAME: &'static str = "path_secret_map:replay_detected_packet_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an ReplayDetected packet was received"]
    pub struct ReplayDetectedPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayDetectedPacketReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayDetectedPacketReceived");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayDetectedPacketReceived<'a> {
        const NAME: &'static str = "path_secret_map:replay_detected_packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an StaleKey packet was authentic and processed"]
    pub struct ReplayDetectedPacketAccepted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
        pub key_id: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayDetectedPacketAccepted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayDetectedPacketAccepted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("key_id", &self.key_id);
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayDetectedPacketAccepted<'a> {
        const NAME: &'static str = "path_secret_map:replay_detected_packet_accepted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an ReplayDetected packet was rejected as invalid"]
    pub struct ReplayDetectedPacketRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayDetectedPacketRejected<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayDetectedPacketRejected");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayDetectedPacketRejected<'a> {
        const NAME: &'static str = "path_secret_map:replay_detected_packet_rejected";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an ReplayDetected packet was dropped due to a missing entry"]
    pub struct ReplayDetectedPacketDropped<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ReplayDetectedPacketDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ReplayDetectedPacketDropped");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for ReplayDetectedPacketDropped<'a> {
        const NAME: &'static str = "path_secret_map:replay_detected_packet_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an StaleKey packet was sent"]
    pub struct StaleKeyPacketSent<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for StaleKeyPacketSent<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StaleKeyPacketSent");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for StaleKeyPacketSent<'a> {
        const NAME: &'static str = "path_secret_map:stale_key_packet_sent";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an StaleKey packet was received"]
    pub struct StaleKeyPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for StaleKeyPacketReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StaleKeyPacketReceived");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for StaleKeyPacketReceived<'a> {
        const NAME: &'static str = "path_secret_map:stale_key_packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an StaleKey packet was authentic and processed"]
    pub struct StaleKeyPacketAccepted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for StaleKeyPacketAccepted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StaleKeyPacketAccepted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for StaleKeyPacketAccepted<'a> {
        const NAME: &'static str = "path_secret_map:stale_key_packet_accepted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an StaleKey packet was rejected as invalid"]
    pub struct StaleKeyPacketRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for StaleKeyPacketRejected<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StaleKeyPacketRejected");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for StaleKeyPacketRejected<'a> {
        const NAME: &'static str = "path_secret_map:stale_key_packet_rejected";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when an StaleKey packet was dropped due to a missing entry"]
    pub struct StaleKeyPacketDropped<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for StaleKeyPacketDropped<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("StaleKeyPacketDropped");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.finish()
        }
    }
    impl<'a> Event for StaleKeyPacketDropped<'a> {
        const NAME: &'static str = "path_secret_map:stale_key_packet_dropped";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the cache is accessed by peer address"]
    #[doc = ""]
    #[doc = " This can be used to track cache hit ratios"]
    pub struct PathSecretMapAddressCacheAccessed<'a> {
        pub peer_address: SocketAddress<'a>,
        pub hit: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapAddressCacheAccessed<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapAddressCacheAccessed");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("hit", &self.hit);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapAddressCacheAccessed<'a> {
        const NAME: &'static str = "path_secret_map:address_cache_accessed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the cache is accessed by peer address successfully"]
    #[doc = ""]
    #[doc = " Provides more information about the accessed entry."]
    pub struct PathSecretMapAddressCacheAccessedHit<'a> {
        pub peer_address: SocketAddress<'a>,
        pub age: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapAddressCacheAccessedHit<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapAddressCacheAccessedHit");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("age", &self.age);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapAddressCacheAccessedHit<'a> {
        const NAME: &'static str = "path_secret_map:address_cache_accessed_entry";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the cache is accessed by path secret ID"]
    #[doc = ""]
    #[doc = " This can be used to track cache hit ratios"]
    pub struct PathSecretMapIdCacheAccessed<'a> {
        pub credential_id: &'a [u8],
        pub hit: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapIdCacheAccessed<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapIdCacheAccessed");
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("hit", &self.hit);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapIdCacheAccessed<'a> {
        const NAME: &'static str = "path_secret_map:id_cache_accessed";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the cache is accessed by path secret ID successfully"]
    #[doc = ""]
    #[doc = " Provides more information about the accessed entry."]
    pub struct PathSecretMapIdCacheAccessedHit<'a> {
        pub credential_id: &'a [u8],
        pub age: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for PathSecretMapIdCacheAccessedHit<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapIdCacheAccessedHit");
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("age", &self.age);
            fmt.finish()
        }
    }
    impl<'a> Event for PathSecretMapIdCacheAccessedHit<'a> {
        const NAME: &'static str = "path_secret_map:id_cache_accessed_entry";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Emitted when the cleaner task performed a single cycle"]
    #[doc = ""]
    #[doc = " This can be used to track cache utilization"]
    pub struct PathSecretMapCleanerCycled {
        #[doc = " The number of Path Secret ID entries left after the cleaning cycle"]
        pub id_entries: usize,
        #[doc = " The number of Path Secret ID entries that were retired in the cycle"]
        pub id_entries_retired: usize,
        #[doc = " Count of entries accessed since the last cycle"]
        pub id_entries_active: usize,
        #[doc = " The utilization percentage of the active number of entries after the cycle"]
        pub id_entries_active_utilization: f32,
        #[doc = " The utilization percentage of the available number of entries after the cycle"]
        pub id_entries_utilization: f32,
        #[doc = " The utilization percentage of the available number of entries before the cycle"]
        pub id_entries_initial_utilization: f32,
        #[doc = " The number of SocketAddress entries left after the cleaning cycle"]
        pub address_entries: usize,
        #[doc = " Count of entries accessed since the last cycle"]
        pub address_entries_active: usize,
        #[doc = " The utilization percentage of the active number of entries after the cycle"]
        pub address_entries_active_utilization: f32,
        #[doc = " The number of SocketAddress entries that were retired in the cycle"]
        pub address_entries_retired: usize,
        #[doc = " The utilization percentage of the available number of address entries after the cycle"]
        pub address_entries_utilization: f32,
        #[doc = " The utilization percentage of the available number of address entries before the cycle"]
        pub address_entries_initial_utilization: f32,
        #[doc = " The number of handshake requests that are pending after the cleaning cycle"]
        pub handshake_requests: usize,
        #[doc = " The number of handshake requests that were retired in the cycle"]
        pub handshake_requests_retired: usize,
        #[doc = " How long we kept the handshake lock held (this blocks completing handshakes)."]
        pub handshake_lock_duration: core::time::Duration,
        #[doc = " Total duration of a cycle."]
        pub duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PathSecretMapCleanerCycled {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapCleanerCycled");
            fmt.field("id_entries", &self.id_entries);
            fmt.field("id_entries_retired", &self.id_entries_retired);
            fmt.field("id_entries_active", &self.id_entries_active);
            fmt.field(
                "id_entries_active_utilization",
                &self.id_entries_active_utilization,
            );
            fmt.field("id_entries_utilization", &self.id_entries_utilization);
            fmt.field(
                "id_entries_initial_utilization",
                &self.id_entries_initial_utilization,
            );
            fmt.field("address_entries", &self.address_entries);
            fmt.field("address_entries_active", &self.address_entries_active);
            fmt.field(
                "address_entries_active_utilization",
                &self.address_entries_active_utilization,
            );
            fmt.field("address_entries_retired", &self.address_entries_retired);
            fmt.field(
                "address_entries_utilization",
                &self.address_entries_utilization,
            );
            fmt.field(
                "address_entries_initial_utilization",
                &self.address_entries_initial_utilization,
            );
            fmt.field("handshake_requests", &self.handshake_requests);
            fmt.field(
                "handshake_requests_retired",
                &self.handshake_requests_retired,
            );
            fmt.field("handshake_lock_duration", &self.handshake_lock_duration);
            fmt.field("duration", &self.duration);
            fmt.finish()
        }
    }
    impl Event for PathSecretMapCleanerCycled {
        const NAME: &'static str = "path_secret_map:cleaner_cycled";
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
}
pub mod tracing {
    #![doc = r" This module contains event integration with [`tracing`](https://docs.rs/tracing)"]
    use super::api;
    #[doc = r" Emits events with [`tracing`](https://docs.rs/tracing)"]
    #[derive(Clone, Debug)]
    pub struct Subscriber {
        root: tracing::Span,
    }
    impl Default for Subscriber {
        fn default() -> Self {
            let root = tracing :: span ! (target : "s2n_quic_dc" , tracing :: Level :: DEBUG , "s2n_quic_dc");
            Self { root }
        }
    }
    impl Subscriber {
        fn parent<M: crate::event::Meta>(&self, _meta: &M) -> Option<tracing::Id> {
            self.root.id()
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = tracing::Span;
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            let parent = self.parent(meta);
            tracing :: span ! (target : "s2n_quic_dc" , parent : parent , tracing :: Level :: DEBUG , "conn" , id = meta . id)
        }
        #[inline]
        fn on_acceptor_tcp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStarted,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpStarted {
                id,
                local_address,
                backlog,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_started" , parent : parent , tracing :: Level :: DEBUG , { id = tracing :: field :: debug (id) , local_address = tracing :: field :: debug (local_address) , backlog = tracing :: field :: debug (backlog) });
        }
        #[inline]
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpLoopIterationCompleted,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpLoopIterationCompleted {
                pending_streams,
                slots_idle,
                slot_utilization,
                processing_duration,
                max_sojourn_time,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_loop_iteration_completed" , parent : parent , tracing :: Level :: DEBUG , { pending_streams = tracing :: field :: debug (pending_streams) , slots_idle = tracing :: field :: debug (slots_idle) , slot_utilization = tracing :: field :: debug (slot_utilization) , processing_duration = tracing :: field :: debug (processing_duration) , max_sojourn_time = tracing :: field :: debug (max_sojourn_time) });
        }
        #[inline]
        fn on_acceptor_tcp_fresh_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshEnqueued,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpFreshEnqueued { remote_address } = event;
            tracing :: event ! (target : "acceptor_tcp_fresh_enqueued" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) });
        }
        #[inline]
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshBatchCompleted,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpFreshBatchCompleted {
                enqueued,
                dropped,
                errored,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_fresh_batch_completed" , parent : parent , tracing :: Level :: DEBUG , { enqueued = tracing :: field :: debug (enqueued) , dropped = tracing :: field :: debug (dropped) , errored = tracing :: field :: debug (errored) });
        }
        #[inline]
        fn on_acceptor_tcp_stream_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamDropped,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpStreamDropped {
                remote_address,
                reason,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_stream_dropped" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_acceptor_tcp_stream_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamReplaced,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpStreamReplaced {
                remote_address,
                sojourn_time,
                buffer_len,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_stream_replaced" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , sojourn_time = tracing :: field :: debug (sojourn_time) , buffer_len = tracing :: field :: debug (buffer_len) });
        }
        #[inline]
        fn on_acceptor_tcp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpPacketReceived {
                remote_address,
                credential_id,
                stream_id,
                payload_len,
                is_fin,
                is_fin_known,
                sojourn_time,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_packet_received" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , credential_id = tracing :: field :: debug (credential_id) , stream_id = tracing :: field :: debug (stream_id) , payload_len = tracing :: field :: debug (payload_len) , is_fin = tracing :: field :: debug (is_fin) , is_fin_known = tracing :: field :: debug (is_fin_known) , sojourn_time = tracing :: field :: debug (sojourn_time) });
        }
        #[inline]
        fn on_acceptor_tcp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketDropped,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpPacketDropped {
                remote_address,
                reason,
                sojourn_time,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , reason = tracing :: field :: debug (reason) , sojourn_time = tracing :: field :: debug (sojourn_time) });
        }
        #[inline]
        fn on_acceptor_tcp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamEnqueued,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
                blocked_count,
            } = event;
            tracing :: event ! (target : "acceptor_tcp_stream_enqueued" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , credential_id = tracing :: field :: debug (credential_id) , stream_id = tracing :: field :: debug (stream_id) , sojourn_time = tracing :: field :: debug (sojourn_time) , blocked_count = tracing :: field :: debug (blocked_count) });
        }
        #[inline]
        fn on_acceptor_tcp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpIoError,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorTcpIoError { error } = event;
            tracing :: event ! (target : "acceptor_tcp_io_error" , parent : parent , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) });
        }
        #[inline]
        fn on_acceptor_udp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStarted,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorUdpStarted { id, local_address } = event;
            tracing :: event ! (target : "acceptor_udp_started" , parent : parent , tracing :: Level :: DEBUG , { id = tracing :: field :: debug (id) , local_address = tracing :: field :: debug (local_address) });
        }
        #[inline]
        fn on_acceptor_udp_datagram_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpDatagramReceived,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorUdpDatagramReceived {
                remote_address,
                len,
            } = event;
            tracing :: event ! (target : "acceptor_udp_datagram_received" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , len = tracing :: field :: debug (len) });
        }
        #[inline]
        fn on_acceptor_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorUdpPacketReceived {
                remote_address,
                credential_id,
                stream_id,
                payload_len,
                is_zero_offset,
                is_retransmission,
                is_fin,
                is_fin_known,
            } = event;
            tracing :: event ! (target : "acceptor_udp_packet_received" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , credential_id = tracing :: field :: debug (credential_id) , stream_id = tracing :: field :: debug (stream_id) , payload_len = tracing :: field :: debug (payload_len) , is_zero_offset = tracing :: field :: debug (is_zero_offset) , is_retransmission = tracing :: field :: debug (is_retransmission) , is_fin = tracing :: field :: debug (is_fin) , is_fin_known = tracing :: field :: debug (is_fin_known) });
        }
        #[inline]
        fn on_acceptor_udp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketDropped,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorUdpPacketDropped {
                remote_address,
                reason,
            } = event;
            tracing :: event ! (target : "acceptor_udp_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_acceptor_udp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStreamEnqueued,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorUdpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
            } = event;
            tracing :: event ! (target : "acceptor_udp_stream_enqueued" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , credential_id = tracing :: field :: debug (credential_id) , stream_id = tracing :: field :: debug (stream_id) });
        }
        #[inline]
        fn on_acceptor_udp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpIoError,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorUdpIoError { error } = event;
            tracing :: event ! (target : "acceptor_udp_io_error" , parent : parent , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) });
        }
        #[inline]
        fn on_acceptor_stream_pruned(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamPruned,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorStreamPruned {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
                reason,
            } = event;
            tracing :: event ! (target : "acceptor_stream_pruned" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , credential_id = tracing :: field :: debug (credential_id) , stream_id = tracing :: field :: debug (stream_id) , sojourn_time = tracing :: field :: debug (sojourn_time) , reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_acceptor_stream_dequeued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamDequeued,
        ) {
            let parent = self.parent(meta);
            let api::AcceptorStreamDequeued {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
            } = event;
            tracing :: event ! (target : "acceptor_stream_dequeued" , parent : parent , tracing :: Level :: DEBUG , { remote_address = tracing :: field :: debug (remote_address) , credential_id = tracing :: field :: debug (credential_id) , stream_id = tracing :: field :: debug (stream_id) , sojourn_time = tracing :: field :: debug (sojourn_time) });
        }
        #[inline]
        fn on_stream_write_flushed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteFlushed,
        ) {
            let id = context.id();
            let api::StreamWriteFlushed {
                provided_len,
                committed_len,
                processing_duration,
            } = event;
            tracing :: event ! (target : "stream_write_flushed" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) , committed_len = tracing :: field :: debug (committed_len) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_stream_write_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteFinFlushed,
        ) {
            let id = context.id();
            let api::StreamWriteFinFlushed {
                provided_len,
                committed_len,
                processing_duration,
            } = event;
            tracing :: event ! (target : "stream_write_fin_flushed" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) , committed_len = tracing :: field :: debug (committed_len) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_stream_write_blocked(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteBlocked,
        ) {
            let id = context.id();
            let api::StreamWriteBlocked {
                provided_len,
                is_fin,
                processing_duration,
            } = event;
            tracing :: event ! (target : "stream_write_blocked" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) , is_fin = tracing :: field :: debug (is_fin) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_stream_write_errored(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteErrored,
        ) {
            let id = context.id();
            let api::StreamWriteErrored {
                provided_len,
                is_fin,
                processing_duration,
                errno,
            } = event;
            tracing :: event ! (target : "stream_write_errored" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) , is_fin = tracing :: field :: debug (is_fin) , processing_duration = tracing :: field :: debug (processing_duration) , errno = tracing :: field :: debug (errno) });
        }
        #[inline]
        fn on_stream_write_key_updated(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteKeyUpdated,
        ) {
            let id = context.id();
            let api::StreamWriteKeyUpdated { key_phase } = event;
            tracing :: event ! (target : "stream_write_key_updated" , parent : id , tracing :: Level :: DEBUG , { key_phase = tracing :: field :: debug (key_phase) });
        }
        #[inline]
        fn on_stream_write_shutdown(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteShutdown,
        ) {
            let id = context.id();
            let api::StreamWriteShutdown {
                buffer_len,
                background,
            } = event;
            tracing :: event ! (target : "stream_write_shutdown" , parent : id , tracing :: Level :: DEBUG , { buffer_len = tracing :: field :: debug (buffer_len) , background = tracing :: field :: debug (background) });
        }
        #[inline]
        fn on_stream_write_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketFlushed,
        ) {
            let id = context.id();
            let api::StreamWriteSocketFlushed {
                provided_len,
                committed_len,
            } = event;
            tracing :: event ! (target : "stream_write_socket_flushed" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) , committed_len = tracing :: field :: debug (committed_len) });
        }
        #[inline]
        fn on_stream_write_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketBlocked,
        ) {
            let id = context.id();
            let api::StreamWriteSocketBlocked { provided_len } = event;
            tracing :: event ! (target : "stream_write_socket_blocked" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) });
        }
        #[inline]
        fn on_stream_write_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketErrored,
        ) {
            let id = context.id();
            let api::StreamWriteSocketErrored {
                provided_len,
                errno,
            } = event;
            tracing :: event ! (target : "stream_write_socket_errored" , parent : id , tracing :: Level :: DEBUG , { provided_len = tracing :: field :: debug (provided_len) , errno = tracing :: field :: debug (errno) });
        }
        #[inline]
        fn on_stream_read_flushed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadFlushed,
        ) {
            let id = context.id();
            let api::StreamReadFlushed {
                capacity,
                committed_len,
                processing_duration,
            } = event;
            tracing :: event ! (target : "stream_read_flushed" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , committed_len = tracing :: field :: debug (committed_len) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_stream_read_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadFinFlushed,
        ) {
            let id = context.id();
            let api::StreamReadFinFlushed {
                capacity,
                processing_duration,
            } = event;
            tracing :: event ! (target : "stream_read_fin_flushed" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_stream_read_blocked(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadBlocked,
        ) {
            let id = context.id();
            let api::StreamReadBlocked {
                capacity,
                processing_duration,
            } = event;
            tracing :: event ! (target : "stream_read_blocked" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , processing_duration = tracing :: field :: debug (processing_duration) });
        }
        #[inline]
        fn on_stream_read_errored(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadErrored,
        ) {
            let id = context.id();
            let api::StreamReadErrored {
                capacity,
                processing_duration,
                errno,
            } = event;
            tracing :: event ! (target : "stream_read_errored" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , processing_duration = tracing :: field :: debug (processing_duration) , errno = tracing :: field :: debug (errno) });
        }
        #[inline]
        fn on_stream_read_key_updated(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadKeyUpdated,
        ) {
            let id = context.id();
            let api::StreamReadKeyUpdated { key_phase } = event;
            tracing :: event ! (target : "stream_read_key_updated" , parent : id , tracing :: Level :: DEBUG , { key_phase = tracing :: field :: debug (key_phase) });
        }
        #[inline]
        fn on_stream_read_shutdown(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadShutdown,
        ) {
            let id = context.id();
            let api::StreamReadShutdown { background } = event;
            tracing :: event ! (target : "stream_read_shutdown" , parent : id , tracing :: Level :: DEBUG , { background = tracing :: field :: debug (background) });
        }
        #[inline]
        fn on_stream_read_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketFlushed,
        ) {
            let id = context.id();
            let api::StreamReadSocketFlushed {
                capacity,
                committed_len,
            } = event;
            tracing :: event ! (target : "stream_read_socket_flushed" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , committed_len = tracing :: field :: debug (committed_len) });
        }
        #[inline]
        fn on_stream_read_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketBlocked,
        ) {
            let id = context.id();
            let api::StreamReadSocketBlocked { capacity } = event;
            tracing :: event ! (target : "stream_read_socket_blocked" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) });
        }
        #[inline]
        fn on_stream_read_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketErrored,
        ) {
            let id = context.id();
            let api::StreamReadSocketErrored { capacity, errno } = event;
            tracing :: event ! (target : "stream_read_socket_errored" , parent : id , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , errno = tracing :: field :: debug (errno) });
        }
        #[inline]
        fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
            let parent = self.parent(meta);
            let api::StreamTcpConnect { error, latency } = event;
            tracing :: event ! (target : "stream_tcp_connect" , parent : parent , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) , latency = tracing :: field :: debug (latency) });
        }
        #[inline]
        fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
            let parent = self.parent(meta);
            let api::StreamConnect {
                error,
                tcp_success,
                handshake_success,
            } = event;
            tracing :: event ! (target : "stream_connect" , parent : parent , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) , tcp_success = tracing :: field :: debug (tcp_success) , handshake_success = tracing :: field :: debug (handshake_success) });
        }
        #[inline]
        fn on_stream_connect_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StreamConnectError,
        ) {
            let parent = self.parent(meta);
            let api::StreamConnectError { reason } = event;
            tracing :: event ! (target : "stream_connect_error" , parent : parent , tracing :: Level :: DEBUG , { reason = tracing :: field :: debug (reason) });
        }
        #[inline]
        fn on_connection_closed(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            let id = context.id();
            let api::ConnectionClosed {} = event;
            tracing :: event ! (target : "connection_closed" , parent : id , tracing :: Level :: DEBUG , { });
        }
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            let parent = self.parent(meta);
            let api::EndpointInitialized {
                acceptor_addr,
                handshake_addr,
                tcp,
                udp,
            } = event;
            tracing :: event ! (target : "endpoint_initialized" , parent : parent , tracing :: Level :: DEBUG , { acceptor_addr = tracing :: field :: debug (acceptor_addr) , handshake_addr = tracing :: field :: debug (handshake_addr) , tcp = tracing :: field :: debug (tcp) , udp = tracing :: field :: debug (udp) });
        }
        #[inline]
        fn on_path_secret_map_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapInitialized,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapInitialized { capacity } = event;
            tracing :: event ! (target : "path_secret_map_initialized" , parent : parent , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) });
        }
        #[inline]
        fn on_path_secret_map_uninitialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapUninitialized,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapUninitialized {
                capacity,
                entries,
                lifetime,
            } = event;
            tracing :: event ! (target : "path_secret_map_uninitialized" , parent : parent , tracing :: Level :: DEBUG , { capacity = tracing :: field :: debug (capacity) , entries = tracing :: field :: debug (entries) , lifetime = tracing :: field :: debug (lifetime) });
        }
        #[inline]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapBackgroundHandshakeRequested,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapBackgroundHandshakeRequested { peer_address } = event;
            tracing :: event ! (target : "path_secret_map_background_handshake_requested" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) });
        }
        #[inline]
        fn on_path_secret_map_entry_inserted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryInserted,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapEntryInserted {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "path_secret_map_entry_inserted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_path_secret_map_entry_ready(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReady,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapEntryReady {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "path_secret_map_entry_ready" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_path_secret_map_entry_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReplaced,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapEntryReplaced {
                peer_address,
                new_credential_id,
                previous_credential_id,
            } = event;
            tracing :: event ! (target : "path_secret_map_entry_replaced" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , new_credential_id = tracing :: field :: debug (new_credential_id) , previous_credential_id = tracing :: field :: debug (previous_credential_id) });
        }
        #[inline]
        fn on_path_secret_map_id_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdEntryEvicted,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapIdEntryEvicted {
                peer_address,
                credential_id,
                age,
            } = event;
            tracing :: event ! (target : "path_secret_map_id_entry_evicted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) , age = tracing :: field :: debug (age) });
        }
        #[inline]
        fn on_path_secret_map_address_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressEntryEvicted,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapAddressEntryEvicted {
                peer_address,
                credential_id,
                age,
            } = event;
            tracing :: event ! (target : "path_secret_map_address_entry_evicted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) , age = tracing :: field :: debug (age) });
        }
        #[inline]
        fn on_unknown_path_secret_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketSent,
        ) {
            let parent = self.parent(meta);
            let api::UnknownPathSecretPacketSent {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "unknown_path_secret_packet_sent" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_unknown_path_secret_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::UnknownPathSecretPacketReceived {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "unknown_path_secret_packet_received" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_unknown_path_secret_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketAccepted,
        ) {
            let parent = self.parent(meta);
            let api::UnknownPathSecretPacketAccepted {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "unknown_path_secret_packet_accepted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_unknown_path_secret_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketRejected,
        ) {
            let parent = self.parent(meta);
            let api::UnknownPathSecretPacketRejected {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "unknown_path_secret_packet_rejected" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_unknown_path_secret_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketDropped,
        ) {
            let parent = self.parent(meta);
            let api::UnknownPathSecretPacketDropped {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "unknown_path_secret_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
            let parent = self.parent(meta);
            let api::KeyAccepted {
                credential_id,
                key_id,
                gap,
                forward_shift,
            } = event;
            tracing :: event ! (target : "key_accepted" , parent : parent , tracing :: Level :: DEBUG , { credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id) , gap = tracing :: field :: debug (gap) , forward_shift = tracing :: field :: debug (forward_shift) });
        }
        #[inline]
        fn on_replay_definitely_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDefinitelyDetected,
        ) {
            let parent = self.parent(meta);
            let api::ReplayDefinitelyDetected {
                credential_id,
                key_id,
            } = event;
            tracing :: event ! (target : "replay_definitely_detected" , parent : parent , tracing :: Level :: DEBUG , { credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id) });
        }
        #[inline]
        fn on_replay_potentially_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayPotentiallyDetected,
        ) {
            let parent = self.parent(meta);
            let api::ReplayPotentiallyDetected {
                credential_id,
                key_id,
                gap,
            } = event;
            tracing :: event ! (target : "replay_potentially_detected" , parent : parent , tracing :: Level :: DEBUG , { credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id) , gap = tracing :: field :: debug (gap) });
        }
        #[inline]
        fn on_replay_detected_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketSent,
        ) {
            let parent = self.parent(meta);
            let api::ReplayDetectedPacketSent {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "replay_detected_packet_sent" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_replay_detected_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::ReplayDetectedPacketReceived {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "replay_detected_packet_received" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_replay_detected_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketAccepted,
        ) {
            let parent = self.parent(meta);
            let api::ReplayDetectedPacketAccepted {
                peer_address,
                credential_id,
                key_id,
            } = event;
            tracing :: event ! (target : "replay_detected_packet_accepted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id) });
        }
        #[inline]
        fn on_replay_detected_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketRejected,
        ) {
            let parent = self.parent(meta);
            let api::ReplayDetectedPacketRejected {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "replay_detected_packet_rejected" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_replay_detected_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketDropped,
        ) {
            let parent = self.parent(meta);
            let api::ReplayDetectedPacketDropped {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "replay_detected_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_stale_key_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketSent,
        ) {
            let parent = self.parent(meta);
            let api::StaleKeyPacketSent {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "stale_key_packet_sent" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_stale_key_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::StaleKeyPacketReceived {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "stale_key_packet_received" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_stale_key_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketAccepted,
        ) {
            let parent = self.parent(meta);
            let api::StaleKeyPacketAccepted {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "stale_key_packet_accepted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_stale_key_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketRejected,
        ) {
            let parent = self.parent(meta);
            let api::StaleKeyPacketRejected {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "stale_key_packet_rejected" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_stale_key_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketDropped,
        ) {
            let parent = self.parent(meta);
            let api::StaleKeyPacketDropped {
                peer_address,
                credential_id,
            } = event;
            tracing :: event ! (target : "stale_key_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) });
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessed,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapAddressCacheAccessed { peer_address, hit } = event;
            tracing :: event ! (target : "path_secret_map_address_cache_accessed" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , hit = tracing :: field :: debug (hit) });
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessedHit,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapAddressCacheAccessedHit { peer_address, age } = event;
            tracing :: event ! (target : "path_secret_map_address_cache_accessed_hit" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , age = tracing :: field :: debug (age) });
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessed,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapIdCacheAccessed { credential_id, hit } = event;
            tracing :: event ! (target : "path_secret_map_id_cache_accessed" , parent : parent , tracing :: Level :: DEBUG , { credential_id = tracing :: field :: debug (credential_id) , hit = tracing :: field :: debug (hit) });
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessedHit,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapIdCacheAccessedHit { credential_id, age } = event;
            tracing :: event ! (target : "path_secret_map_id_cache_accessed_hit" , parent : parent , tracing :: Level :: DEBUG , { credential_id = tracing :: field :: debug (credential_id) , age = tracing :: field :: debug (age) });
        }
        #[inline]
        fn on_path_secret_map_cleaner_cycled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapCleanerCycled,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapCleanerCycled {
                id_entries,
                id_entries_retired,
                id_entries_active,
                id_entries_active_utilization,
                id_entries_utilization,
                id_entries_initial_utilization,
                address_entries,
                address_entries_active,
                address_entries_active_utilization,
                address_entries_retired,
                address_entries_utilization,
                address_entries_initial_utilization,
                handshake_requests,
                handshake_requests_retired,
                handshake_lock_duration,
                duration,
            } = event;
            tracing :: event ! (target : "path_secret_map_cleaner_cycled" , parent : parent , tracing :: Level :: DEBUG , { id_entries = tracing :: field :: debug (id_entries) , id_entries_retired = tracing :: field :: debug (id_entries_retired) , id_entries_active = tracing :: field :: debug (id_entries_active) , id_entries_active_utilization = tracing :: field :: debug (id_entries_active_utilization) , id_entries_utilization = tracing :: field :: debug (id_entries_utilization) , id_entries_initial_utilization = tracing :: field :: debug (id_entries_initial_utilization) , address_entries = tracing :: field :: debug (address_entries) , address_entries_active = tracing :: field :: debug (address_entries_active) , address_entries_active_utilization = tracing :: field :: debug (address_entries_active_utilization) , address_entries_retired = tracing :: field :: debug (address_entries_retired) , address_entries_utilization = tracing :: field :: debug (address_entries_utilization) , address_entries_initial_utilization = tracing :: field :: debug (address_entries_initial_utilization) , handshake_requests = tracing :: field :: debug (handshake_requests) , handshake_requests_retired = tracing :: field :: debug (handshake_requests_retired) , handshake_lock_duration = tracing :: field :: debug (handshake_lock_duration) , duration = tracing :: field :: debug (duration) });
        }
    }
}
pub mod builder {
    use super::*;
    pub use s2n_quic_core::event::builder::{EndpointType, SocketAddress, Subject};
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a TCP acceptor is started"]
    pub struct AcceptorTcpStarted<'a> {
        #[doc = " The id of the acceptor worker"]
        pub id: usize,
        #[doc = " The local address of the acceptor"]
        pub local_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The backlog size"]
        pub backlog: usize,
    }
    impl<'a> IntoEvent<api::AcceptorTcpStarted<'a>> for AcceptorTcpStarted<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpStarted<'a> {
            let AcceptorTcpStarted {
                id,
                local_address,
                backlog,
            } = self;
            api::AcceptorTcpStarted {
                id: id.into_event(),
                local_address: local_address.into_event(),
                backlog: backlog.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a TCP acceptor completes a single iteration of the event loop"]
    pub struct AcceptorTcpLoopIterationCompleted {
        #[doc = " The number of streams that are waiting on initial packets"]
        pub pending_streams: usize,
        #[doc = " The number of slots that are not currently processing a stream"]
        pub slots_idle: usize,
        #[doc = " The percentage of slots currently processing streams"]
        pub slot_utilization: f32,
        #[doc = " The amount of time it took to complete the iteration"]
        pub processing_duration: core::time::Duration,
        #[doc = " The computed max sojourn time that is allowed for streams"]
        #[doc = ""]
        #[doc = " If streams consume more time than this value to initialize, they"]
        #[doc = " may potentially be replaced by more recent streams."]
        pub max_sojourn_time: core::time::Duration,
    }
    impl IntoEvent<api::AcceptorTcpLoopIterationCompleted> for AcceptorTcpLoopIterationCompleted {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpLoopIterationCompleted {
            let AcceptorTcpLoopIterationCompleted {
                pending_streams,
                slots_idle,
                slot_utilization,
                processing_duration,
                max_sojourn_time,
            } = self;
            api::AcceptorTcpLoopIterationCompleted {
                pending_streams: pending_streams.into_event(),
                slots_idle: slots_idle.into_event(),
                slot_utilization: slot_utilization.into_event(),
                processing_duration: processing_duration.into_event(),
                max_sojourn_time: max_sojourn_time.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a fresh TCP stream is enqueued for processing"]
    pub struct AcceptorTcpFreshEnqueued<'a> {
        #[doc = " The remote address of the TCP stream"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
    }
    impl<'a> IntoEvent<api::AcceptorTcpFreshEnqueued<'a>> for AcceptorTcpFreshEnqueued<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpFreshEnqueued<'a> {
            let AcceptorTcpFreshEnqueued { remote_address } = self;
            api::AcceptorTcpFreshEnqueued {
                remote_address: remote_address.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a the TCP acceptor has completed a batch of stream enqueues"]
    pub struct AcceptorTcpFreshBatchCompleted {
        #[doc = " The number of fresh TCP streams enqueued in this batch"]
        pub enqueued: usize,
        #[doc = " The number of fresh TCP streams dropped in this batch due to capacity limits"]
        pub dropped: usize,
        #[doc = " The number of TCP streams that errored in this batch"]
        pub errored: usize,
    }
    impl IntoEvent<api::AcceptorTcpFreshBatchCompleted> for AcceptorTcpFreshBatchCompleted {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpFreshBatchCompleted {
            let AcceptorTcpFreshBatchCompleted {
                enqueued,
                dropped,
                errored,
            } = self;
            api::AcceptorTcpFreshBatchCompleted {
                enqueued: enqueued.into_event(),
                dropped: dropped.into_event(),
                errored: errored.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a TCP stream has been dropped"]
    pub struct AcceptorTcpStreamDropped<'a> {
        #[doc = " The remote address of the TCP stream"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        pub reason: AcceptorTcpStreamDropReason,
    }
    impl<'a> IntoEvent<api::AcceptorTcpStreamDropped<'a>> for AcceptorTcpStreamDropped<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpStreamDropped<'a> {
            let AcceptorTcpStreamDropped {
                remote_address,
                reason,
            } = self;
            api::AcceptorTcpStreamDropped {
                remote_address: remote_address.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a TCP stream has been replaced by another stream"]
    pub struct AcceptorTcpStreamReplaced<'a> {
        #[doc = " The remote address of the stream being replaced"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The amount of time that the stream spent in the accept queue before"]
        #[doc = " being replaced with another"]
        pub sojourn_time: core::time::Duration,
        #[doc = " The amount of bytes buffered on the stream"]
        pub buffer_len: usize,
    }
    impl<'a> IntoEvent<api::AcceptorTcpStreamReplaced<'a>> for AcceptorTcpStreamReplaced<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpStreamReplaced<'a> {
            let AcceptorTcpStreamReplaced {
                remote_address,
                sojourn_time,
                buffer_len,
            } = self;
            api::AcceptorTcpStreamReplaced {
                remote_address: remote_address.into_event(),
                sojourn_time: sojourn_time.into_event(),
                buffer_len: buffer_len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a full packet has been received on the TCP stream"]
    pub struct AcceptorTcpPacketReceived<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The credential ID of the packet"]
        pub credential_id: &'a [u8],
        #[doc = " The stream ID of the packet"]
        pub stream_id: u64,
        #[doc = " The payload length of the packet"]
        pub payload_len: usize,
        #[doc = " If the packet includes the final bytes of the stream"]
        pub is_fin: bool,
        #[doc = " If the packet includes the final offset of the stream"]
        pub is_fin_known: bool,
        #[doc = " The amount of time the TCP stream spent in the queue before receiving"]
        #[doc = " the initial packet"]
        pub sojourn_time: core::time::Duration,
    }
    impl<'a> IntoEvent<api::AcceptorTcpPacketReceived<'a>> for AcceptorTcpPacketReceived<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpPacketReceived<'a> {
            let AcceptorTcpPacketReceived {
                remote_address,
                credential_id,
                stream_id,
                payload_len,
                is_fin,
                is_fin_known,
                sojourn_time,
            } = self;
            api::AcceptorTcpPacketReceived {
                remote_address: remote_address.into_event(),
                credential_id: credential_id.into_event(),
                stream_id: stream_id.into_event(),
                payload_len: payload_len.into_event(),
                is_fin: is_fin.into_event(),
                is_fin_known: is_fin_known.into_event(),
                sojourn_time: sojourn_time.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the TCP acceptor received an invalid initial packet"]
    pub struct AcceptorTcpPacketDropped<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The reason the packet was dropped"]
        pub reason: AcceptorPacketDropReason,
        #[doc = " The amount of time the TCP stream spent in the queue before receiving"]
        #[doc = " an error"]
        pub sojourn_time: core::time::Duration,
    }
    impl<'a> IntoEvent<api::AcceptorTcpPacketDropped<'a>> for AcceptorTcpPacketDropped<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpPacketDropped<'a> {
            let AcceptorTcpPacketDropped {
                remote_address,
                reason,
                sojourn_time,
            } = self;
            api::AcceptorTcpPacketDropped {
                remote_address: remote_address.into_event(),
                reason: reason.into_event(),
                sojourn_time: sojourn_time.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the TCP stream has been enqueued for the application"]
    pub struct AcceptorTcpStreamEnqueued<'a> {
        #[doc = " The address of the stream's peer"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
        #[doc = " The amount of time the TCP stream spent in the queue before being enqueued"]
        pub sojourn_time: core::time::Duration,
        #[doc = " The number of times the stream was blocked on receiving more data"]
        pub blocked_count: usize,
    }
    impl<'a> IntoEvent<api::AcceptorTcpStreamEnqueued<'a>> for AcceptorTcpStreamEnqueued<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpStreamEnqueued<'a> {
            let AcceptorTcpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
                blocked_count,
            } = self;
            api::AcceptorTcpStreamEnqueued {
                remote_address: remote_address.into_event(),
                credential_id: credential_id.into_event(),
                stream_id: stream_id.into_event(),
                sojourn_time: sojourn_time.into_event(),
                blocked_count: blocked_count.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the TCP acceptor encounters an IO error"]
    pub struct AcceptorTcpIoError<'a> {
        #[doc = " The error encountered"]
        pub error: &'a std::io::Error,
    }
    impl<'a> IntoEvent<api::AcceptorTcpIoError<'a>> for AcceptorTcpIoError<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpIoError<'a> {
            let AcceptorTcpIoError { error } = self;
            api::AcceptorTcpIoError {
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a UDP acceptor is started"]
    pub struct AcceptorUdpStarted<'a> {
        #[doc = " The id of the acceptor worker"]
        pub id: usize,
        #[doc = " The local address of the acceptor"]
        pub local_address: SocketAddress<'a>,
    }
    impl<'a> IntoEvent<api::AcceptorUdpStarted<'a>> for AcceptorUdpStarted<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorUdpStarted<'a> {
            let AcceptorUdpStarted { id, local_address } = self;
            api::AcceptorUdpStarted {
                id: id.into_event(),
                local_address: local_address.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a UDP datagram is received by the acceptor"]
    pub struct AcceptorUdpDatagramReceived<'a> {
        #[doc = " The address of the datagram's sender"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The len of the datagram"]
        pub len: usize,
    }
    impl<'a> IntoEvent<api::AcceptorUdpDatagramReceived<'a>> for AcceptorUdpDatagramReceived<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorUdpDatagramReceived<'a> {
            let AcceptorUdpDatagramReceived {
                remote_address,
                len,
            } = self;
            api::AcceptorUdpDatagramReceived {
                remote_address: remote_address.into_event(),
                len: len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the UDP acceptor parsed a packet contained in a datagram"]
    pub struct AcceptorUdpPacketReceived<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The credential ID of the packet"]
        pub credential_id: &'a [u8],
        #[doc = " The stream ID of the packet"]
        pub stream_id: u64,
        #[doc = " The payload length of the packet"]
        pub payload_len: usize,
        #[doc = " If the packets is a zero offset in the stream"]
        pub is_zero_offset: bool,
        #[doc = " If the packet is a retransmission"]
        pub is_retransmission: bool,
        #[doc = " If the packet includes the final bytes of the stream"]
        pub is_fin: bool,
        #[doc = " If the packet includes the final offset of the stream"]
        pub is_fin_known: bool,
    }
    impl<'a> IntoEvent<api::AcceptorUdpPacketReceived<'a>> for AcceptorUdpPacketReceived<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorUdpPacketReceived<'a> {
            let AcceptorUdpPacketReceived {
                remote_address,
                credential_id,
                stream_id,
                payload_len,
                is_zero_offset,
                is_retransmission,
                is_fin,
                is_fin_known,
            } = self;
            api::AcceptorUdpPacketReceived {
                remote_address: remote_address.into_event(),
                credential_id: credential_id.into_event(),
                stream_id: stream_id.into_event(),
                payload_len: payload_len.into_event(),
                is_zero_offset: is_zero_offset.into_event(),
                is_retransmission: is_retransmission.into_event(),
                is_fin: is_fin.into_event(),
                is_fin_known: is_fin_known.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the UDP acceptor received an invalid initial packet"]
    pub struct AcceptorUdpPacketDropped<'a> {
        #[doc = " The address of the packet's sender"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The reason the packet was dropped"]
        pub reason: AcceptorPacketDropReason,
    }
    impl<'a> IntoEvent<api::AcceptorUdpPacketDropped<'a>> for AcceptorUdpPacketDropped<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorUdpPacketDropped<'a> {
            let AcceptorUdpPacketDropped {
                remote_address,
                reason,
            } = self;
            api::AcceptorUdpPacketDropped {
                remote_address: remote_address.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the UDP stream has been enqueued for the application"]
    pub struct AcceptorUdpStreamEnqueued<'a> {
        #[doc = " The address of the stream's peer"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
    }
    impl<'a> IntoEvent<api::AcceptorUdpStreamEnqueued<'a>> for AcceptorUdpStreamEnqueued<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorUdpStreamEnqueued<'a> {
            let AcceptorUdpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
            } = self;
            api::AcceptorUdpStreamEnqueued {
                remote_address: remote_address.into_event(),
                credential_id: credential_id.into_event(),
                stream_id: stream_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the UDP acceptor encounters an IO error"]
    pub struct AcceptorUdpIoError<'a> {
        #[doc = " The error encountered"]
        pub error: &'a std::io::Error,
    }
    impl<'a> IntoEvent<api::AcceptorUdpIoError<'a>> for AcceptorUdpIoError<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorUdpIoError<'a> {
            let AcceptorUdpIoError { error } = self;
            api::AcceptorUdpIoError {
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a stream has been pruned"]
    pub struct AcceptorStreamPruned<'a> {
        #[doc = " The remote address of the stream"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
        #[doc = " The amount of time that the stream spent in the accept queue before"]
        #[doc = " being pruned"]
        pub sojourn_time: core::time::Duration,
        pub reason: AcceptorStreamPruneReason,
    }
    impl<'a> IntoEvent<api::AcceptorStreamPruned<'a>> for AcceptorStreamPruned<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorStreamPruned<'a> {
            let AcceptorStreamPruned {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
                reason,
            } = self;
            api::AcceptorStreamPruned {
                remote_address: remote_address.into_event(),
                credential_id: credential_id.into_event(),
                stream_id: stream_id.into_event(),
                sojourn_time: sojourn_time.into_event(),
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a stream has been dequeued by the application"]
    pub struct AcceptorStreamDequeued<'a> {
        #[doc = " The remote address of the stream"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " The credential ID of the stream"]
        pub credential_id: &'a [u8],
        #[doc = " The ID of the stream"]
        pub stream_id: u64,
        #[doc = " The amount of time that the stream spent in the accept queue before"]
        #[doc = " being dequeued"]
        pub sojourn_time: core::time::Duration,
    }
    impl<'a> IntoEvent<api::AcceptorStreamDequeued<'a>> for AcceptorStreamDequeued<'a> {
        #[inline]
        fn into_event(self) -> api::AcceptorStreamDequeued<'a> {
            let AcceptorStreamDequeued {
                remote_address,
                credential_id,
                stream_id,
                sojourn_time,
            } = self;
            api::AcceptorStreamDequeued {
                remote_address: remote_address.into_event(),
                credential_id: credential_id.into_event(),
                stream_id: stream_id.into_event(),
                sojourn_time: sojourn_time.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum AcceptorTcpStreamDropReason {
        #[doc = " There were more streams in the TCP backlog than the userspace queue can store"]
        FreshQueueAtCapacity,
        #[doc = " There are no available slots for processing"]
        SlotsAtCapacity,
    }
    impl IntoEvent<api::AcceptorTcpStreamDropReason> for AcceptorTcpStreamDropReason {
        #[inline]
        fn into_event(self) -> api::AcceptorTcpStreamDropReason {
            use api::AcceptorTcpStreamDropReason::*;
            match self {
                Self::FreshQueueAtCapacity => FreshQueueAtCapacity {},
                Self::SlotsAtCapacity => SlotsAtCapacity {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum AcceptorStreamPruneReason {
        MaxSojournTimeExceeded,
        AcceptQueueCapacityExceeded,
    }
    impl IntoEvent<api::AcceptorStreamPruneReason> for AcceptorStreamPruneReason {
        #[inline]
        fn into_event(self) -> api::AcceptorStreamPruneReason {
            use api::AcceptorStreamPruneReason::*;
            match self {
                Self::MaxSojournTimeExceeded => MaxSojournTimeExceeded {},
                Self::AcceptQueueCapacityExceeded => AcceptQueueCapacityExceeded {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub enum AcceptorPacketDropReason {
        UnexpectedEof,
        UnexpectedBytes,
        LengthCapacityExceeded,
        InvariantViolation { message: &'static str },
    }
    impl IntoEvent<api::AcceptorPacketDropReason> for AcceptorPacketDropReason {
        #[inline]
        fn into_event(self) -> api::AcceptorPacketDropReason {
            use api::AcceptorPacketDropReason::*;
            match self {
                Self::UnexpectedEof => UnexpectedEof {},
                Self::UnexpectedBytes => UnexpectedBytes {},
                Self::LengthCapacityExceeded => LengthCapacityExceeded {},
                Self::InvariantViolation { message } => InvariantViolation {
                    message: message.into_event(),
                },
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionMeta {
        pub id: u64,
        pub timestamp: Timestamp,
    }
    impl IntoEvent<api::ConnectionMeta> for ConnectionMeta {
        #[inline]
        fn into_event(self) -> api::ConnectionMeta {
            let ConnectionMeta { id, timestamp } = self;
            api::ConnectionMeta {
                id: id.into_event(),
                timestamp: timestamp.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointMeta {
        pub timestamp: Timestamp,
    }
    impl IntoEvent<api::EndpointMeta> for EndpointMeta {
        #[inline]
        fn into_event(self) -> api::EndpointMeta {
            let EndpointMeta { timestamp } = self;
            api::EndpointMeta {
                timestamp: timestamp.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionInfo {}
    impl IntoEvent<api::ConnectionInfo> for ConnectionInfo {
        #[inline]
        fn into_event(self) -> api::ConnectionInfo {
            let ConnectionInfo {} = self;
            api::ConnectionInfo {}
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteFlushed {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " The amount that was written"]
        pub committed_len: usize,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::StreamWriteFlushed> for StreamWriteFlushed {
        #[inline]
        fn into_event(self) -> api::StreamWriteFlushed {
            let StreamWriteFlushed {
                provided_len,
                committed_len,
                processing_duration,
            } = self;
            api::StreamWriteFlushed {
                provided_len: provided_len.into_event(),
                committed_len: committed_len.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteFinFlushed {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " The amount that was written"]
        pub committed_len: usize,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::StreamWriteFinFlushed> for StreamWriteFinFlushed {
        #[inline]
        fn into_event(self) -> api::StreamWriteFinFlushed {
            let StreamWriteFinFlushed {
                provided_len,
                committed_len,
                processing_duration,
            } = self;
            api::StreamWriteFinFlushed {
                provided_len: provided_len.into_event(),
                committed_len: committed_len.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteBlocked {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " Indicates that the write was the final offset of the stream"]
        pub is_fin: bool,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::StreamWriteBlocked> for StreamWriteBlocked {
        #[inline]
        fn into_event(self) -> api::StreamWriteBlocked {
            let StreamWriteBlocked {
                provided_len,
                is_fin,
                processing_duration,
            } = self;
            api::StreamWriteBlocked {
                provided_len: provided_len.into_event(),
                is_fin: is_fin.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteErrored {
        #[doc = " The number of bytes that the application tried to write"]
        pub provided_len: usize,
        #[doc = " Indicates that the write was the final offset of the stream"]
        pub is_fin: bool,
        #[doc = " The amount of time it took to process the write request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and encryption overhead"]
        pub processing_duration: core::time::Duration,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    impl IntoEvent<api::StreamWriteErrored> for StreamWriteErrored {
        #[inline]
        fn into_event(self) -> api::StreamWriteErrored {
            let StreamWriteErrored {
                provided_len,
                is_fin,
                processing_duration,
                errno,
            } = self;
            api::StreamWriteErrored {
                provided_len: provided_len.into_event(),
                is_fin: is_fin.into_event(),
                processing_duration: processing_duration.into_event(),
                errno: errno.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteKeyUpdated {
        pub key_phase: u8,
    }
    impl IntoEvent<api::StreamWriteKeyUpdated> for StreamWriteKeyUpdated {
        #[inline]
        fn into_event(self) -> api::StreamWriteKeyUpdated {
            let StreamWriteKeyUpdated { key_phase } = self;
            api::StreamWriteKeyUpdated {
                key_phase: key_phase.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteShutdown {
        #[doc = " The number of bytes in the send buffer at the time of shutdown"]
        pub buffer_len: usize,
        #[doc = " If the stream required a background task to drive the stream shutdown"]
        pub background: bool,
    }
    impl IntoEvent<api::StreamWriteShutdown> for StreamWriteShutdown {
        #[inline]
        fn into_event(self) -> api::StreamWriteShutdown {
            let StreamWriteShutdown {
                buffer_len,
                background,
            } = self;
            api::StreamWriteShutdown {
                buffer_len: buffer_len.into_event(),
                background: background.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteSocketFlushed {
        #[doc = " The number of bytes that the stream tried to write to the socket"]
        pub provided_len: usize,
        #[doc = " The amount that was written"]
        pub committed_len: usize,
    }
    impl IntoEvent<api::StreamWriteSocketFlushed> for StreamWriteSocketFlushed {
        #[inline]
        fn into_event(self) -> api::StreamWriteSocketFlushed {
            let StreamWriteSocketFlushed {
                provided_len,
                committed_len,
            } = self;
            api::StreamWriteSocketFlushed {
                provided_len: provided_len.into_event(),
                committed_len: committed_len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteSocketBlocked {
        #[doc = " The number of bytes that the stream tried to write to the socket"]
        pub provided_len: usize,
    }
    impl IntoEvent<api::StreamWriteSocketBlocked> for StreamWriteSocketBlocked {
        #[inline]
        fn into_event(self) -> api::StreamWriteSocketBlocked {
            let StreamWriteSocketBlocked { provided_len } = self;
            api::StreamWriteSocketBlocked {
                provided_len: provided_len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamWriteSocketErrored {
        #[doc = " The number of bytes that the stream tried to write to the socket"]
        pub provided_len: usize,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    impl IntoEvent<api::StreamWriteSocketErrored> for StreamWriteSocketErrored {
        #[inline]
        fn into_event(self) -> api::StreamWriteSocketErrored {
            let StreamWriteSocketErrored {
                provided_len,
                errno,
            } = self;
            api::StreamWriteSocketErrored {
                provided_len: provided_len.into_event(),
                errno: errno.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadFlushed {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount that was read into the provided buffer"]
        pub committed_len: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::StreamReadFlushed> for StreamReadFlushed {
        #[inline]
        fn into_event(self) -> api::StreamReadFlushed {
            let StreamReadFlushed {
                capacity,
                committed_len,
                processing_duration,
            } = self;
            api::StreamReadFlushed {
                capacity: capacity.into_event(),
                committed_len: committed_len.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadFinFlushed {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::StreamReadFinFlushed> for StreamReadFinFlushed {
        #[inline]
        fn into_event(self) -> api::StreamReadFinFlushed {
            let StreamReadFinFlushed {
                capacity,
                processing_duration,
            } = self;
            api::StreamReadFinFlushed {
                capacity: capacity.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadBlocked {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
    }
    impl IntoEvent<api::StreamReadBlocked> for StreamReadBlocked {
        #[inline]
        fn into_event(self) -> api::StreamReadBlocked {
            let StreamReadBlocked {
                capacity,
                processing_duration,
            } = self;
            api::StreamReadBlocked {
                capacity: capacity.into_event(),
                processing_duration: processing_duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadErrored {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount of time it took to process the read request"]
        #[doc = ""]
        #[doc = " Note that this includes both any syscall and decryption overhead"]
        pub processing_duration: core::time::Duration,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    impl IntoEvent<api::StreamReadErrored> for StreamReadErrored {
        #[inline]
        fn into_event(self) -> api::StreamReadErrored {
            let StreamReadErrored {
                capacity,
                processing_duration,
                errno,
            } = self;
            api::StreamReadErrored {
                capacity: capacity.into_event(),
                processing_duration: processing_duration.into_event(),
                errno: errno.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadKeyUpdated {
        pub key_phase: u8,
    }
    impl IntoEvent<api::StreamReadKeyUpdated> for StreamReadKeyUpdated {
        #[inline]
        fn into_event(self) -> api::StreamReadKeyUpdated {
            let StreamReadKeyUpdated { key_phase } = self;
            api::StreamReadKeyUpdated {
                key_phase: key_phase.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadShutdown {
        #[doc = " If the stream required a background task to drive the stream shutdown"]
        pub background: bool,
    }
    impl IntoEvent<api::StreamReadShutdown> for StreamReadShutdown {
        #[inline]
        fn into_event(self) -> api::StreamReadShutdown {
            let StreamReadShutdown { background } = self;
            api::StreamReadShutdown {
                background: background.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadSocketFlushed {
        #[doc = " The number of bytes that the stream tried to read from the socket"]
        pub capacity: usize,
        #[doc = " The amount that was read into the provided buffer"]
        pub committed_len: usize,
    }
    impl IntoEvent<api::StreamReadSocketFlushed> for StreamReadSocketFlushed {
        #[inline]
        fn into_event(self) -> api::StreamReadSocketFlushed {
            let StreamReadSocketFlushed {
                capacity,
                committed_len,
            } = self;
            api::StreamReadSocketFlushed {
                capacity: capacity.into_event(),
                committed_len: committed_len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadSocketBlocked {
        #[doc = " The number of bytes that the stream tried to read from the socket"]
        pub capacity: usize,
    }
    impl IntoEvent<api::StreamReadSocketBlocked> for StreamReadSocketBlocked {
        #[inline]
        fn into_event(self) -> api::StreamReadSocketBlocked {
            let StreamReadSocketBlocked { capacity } = self;
            api::StreamReadSocketBlocked {
                capacity: capacity.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct StreamReadSocketErrored {
        #[doc = " The number of bytes that the stream tried to read from the socket"]
        pub capacity: usize,
        #[doc = " The system `errno` from the returned error"]
        pub errno: Option<i32>,
    }
    impl IntoEvent<api::StreamReadSocketErrored> for StreamReadSocketErrored {
        #[inline]
        fn into_event(self) -> api::StreamReadSocketErrored {
            let StreamReadSocketErrored { capacity, errno } = self;
            api::StreamReadSocketErrored {
                capacity: capacity.into_event(),
                errno: errno.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Tracks stream connect where dcQUIC owns the TCP connect()."]
    pub struct StreamTcpConnect {
        pub error: bool,
        pub latency: core::time::Duration,
    }
    impl IntoEvent<api::StreamTcpConnect> for StreamTcpConnect {
        #[inline]
        fn into_event(self) -> api::StreamTcpConnect {
            let StreamTcpConnect { error, latency } = self;
            api::StreamTcpConnect {
                error: error.into_event(),
                latency: latency.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Tracks stream connect where dcQUIC owns the TCP connect()."]
    pub struct StreamConnect {
        pub error: bool,
        pub tcp_success: MaybeBoolCounter,
        pub handshake_success: MaybeBoolCounter,
    }
    impl IntoEvent<api::StreamConnect> for StreamConnect {
        #[inline]
        fn into_event(self) -> api::StreamConnect {
            let StreamConnect {
                error,
                tcp_success,
                handshake_success,
            } = self;
            api::StreamConnect {
                error: error.into_event(),
                tcp_success: tcp_success.into_event(),
                handshake_success: handshake_success.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Tracks stream connect errors."]
    #[doc = ""]
    #[doc = " Currently only emitted in cases where dcQUIC owns the TCP connect too."]
    pub struct StreamConnectError {
        pub reason: StreamTcpConnectErrorReason,
    }
    impl IntoEvent<api::StreamConnectError> for StreamConnectError {
        #[inline]
        fn into_event(self) -> api::StreamConnectError {
            let StreamConnectError { reason } = self;
            api::StreamConnectError {
                reason: reason.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ConnectionClosed {}
    impl IntoEvent<api::ConnectionClosed> for ConnectionClosed {
        #[inline]
        fn into_event(self) -> api::ConnectionClosed {
            let ConnectionClosed {} = self;
            api::ConnectionClosed {}
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Used for cases where we are racing multiple futures and exit if any of them fail, and so"]
    #[doc = " recording success is not just a boolean value."]
    pub enum MaybeBoolCounter {
        Success,
        Failure,
        Aborted,
    }
    impl IntoEvent<api::MaybeBoolCounter> for MaybeBoolCounter {
        #[inline]
        fn into_event(self) -> api::MaybeBoolCounter {
            use api::MaybeBoolCounter::*;
            match self {
                Self::Success => Success {},
                Self::Failure => Failure {},
                Self::Aborted => Aborted {},
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Note that there's no guarantee of a particular reason if multiple reasons ~simultaneously"]
    #[doc = " terminate the connection."]
    pub enum StreamTcpConnectErrorReason {
        #[doc = " TCP connect failed."]
        TcpConnect,
        #[doc = " Handshake failed to produce credentials."]
        Handshake,
        #[doc = " When the connect future is dropped prior to returning any result."]
        #[doc = ""]
        #[doc = " Usually indicates a timeout in the application."]
        Aborted,
    }
    impl IntoEvent<api::StreamTcpConnectErrorReason> for StreamTcpConnectErrorReason {
        #[inline]
        fn into_event(self) -> api::StreamTcpConnectErrorReason {
            use api::StreamTcpConnectErrorReason::*;
            match self {
                Self::TcpConnect => TcpConnect {},
                Self::Handshake => Handshake {},
                Self::Aborted => Aborted {},
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointInitialized<'a> {
        pub acceptor_addr: SocketAddress<'a>,
        pub handshake_addr: SocketAddress<'a>,
        pub tcp: bool,
        pub udp: bool,
    }
    impl<'a> IntoEvent<api::EndpointInitialized<'a>> for EndpointInitialized<'a> {
        #[inline]
        fn into_event(self) -> api::EndpointInitialized<'a> {
            let EndpointInitialized {
                acceptor_addr,
                handshake_addr,
                tcp,
                udp,
            } = self;
            api::EndpointInitialized {
                acceptor_addr: acceptor_addr.into_event(),
                handshake_addr: handshake_addr.into_event(),
                tcp: tcp.into_event(),
                udp: udp.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PathSecretMapInitialized {
        #[doc = " The capacity of the path secret map"]
        pub capacity: usize,
    }
    impl IntoEvent<api::PathSecretMapInitialized> for PathSecretMapInitialized {
        #[inline]
        fn into_event(self) -> api::PathSecretMapInitialized {
            let PathSecretMapInitialized { capacity } = self;
            api::PathSecretMapInitialized {
                capacity: capacity.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PathSecretMapUninitialized {
        #[doc = " The capacity of the path secret map"]
        pub capacity: usize,
        #[doc = " The number of entries in the map"]
        pub entries: usize,
        pub lifetime: core::time::Duration,
    }
    impl IntoEvent<api::PathSecretMapUninitialized> for PathSecretMapUninitialized {
        #[inline]
        fn into_event(self) -> api::PathSecretMapUninitialized {
            let PathSecretMapUninitialized {
                capacity,
                entries,
                lifetime,
            } = self;
            api::PathSecretMapUninitialized {
                capacity: capacity.into_event(),
                entries: entries.into_event(),
                lifetime: lifetime.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a background handshake is requested"]
    pub struct PathSecretMapBackgroundHandshakeRequested<'a> {
        pub peer_address: SocketAddress<'a>,
    }
    impl<'a> IntoEvent<api::PathSecretMapBackgroundHandshakeRequested<'a>>
        for PathSecretMapBackgroundHandshakeRequested<'a>
    {
        #[inline]
        fn into_event(self) -> api::PathSecretMapBackgroundHandshakeRequested<'a> {
            let PathSecretMapBackgroundHandshakeRequested { peer_address } = self;
            api::PathSecretMapBackgroundHandshakeRequested {
                peer_address: peer_address.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the entry is inserted into the path secret map"]
    pub struct PathSecretMapEntryInserted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::PathSecretMapEntryInserted<'a>> for PathSecretMapEntryInserted<'a> {
        #[inline]
        fn into_event(self) -> api::PathSecretMapEntryInserted<'a> {
            let PathSecretMapEntryInserted {
                peer_address,
                credential_id,
            } = self;
            api::PathSecretMapEntryInserted {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the entry is considered ready for use"]
    pub struct PathSecretMapEntryReady<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::PathSecretMapEntryReady<'a>> for PathSecretMapEntryReady<'a> {
        #[inline]
        fn into_event(self) -> api::PathSecretMapEntryReady<'a> {
            let PathSecretMapEntryReady {
                peer_address,
                credential_id,
            } = self;
            api::PathSecretMapEntryReady {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an entry is replaced by a new one for the same `peer_address`"]
    pub struct PathSecretMapEntryReplaced<'a> {
        pub peer_address: SocketAddress<'a>,
        pub new_credential_id: &'a [u8],
        pub previous_credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::PathSecretMapEntryReplaced<'a>> for PathSecretMapEntryReplaced<'a> {
        #[inline]
        fn into_event(self) -> api::PathSecretMapEntryReplaced<'a> {
            let PathSecretMapEntryReplaced {
                peer_address,
                new_credential_id,
                previous_credential_id,
            } = self;
            api::PathSecretMapEntryReplaced {
                peer_address: peer_address.into_event(),
                new_credential_id: new_credential_id.into_event(),
                previous_credential_id: previous_credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an entry is evicted due to running out of space"]
    pub struct PathSecretMapIdEntryEvicted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
        #[doc = " Time since insertion of this entry"]
        pub age: core::time::Duration,
    }
    impl<'a> IntoEvent<api::PathSecretMapIdEntryEvicted<'a>> for PathSecretMapIdEntryEvicted<'a> {
        #[inline]
        fn into_event(self) -> api::PathSecretMapIdEntryEvicted<'a> {
            let PathSecretMapIdEntryEvicted {
                peer_address,
                credential_id,
                age,
            } = self;
            api::PathSecretMapIdEntryEvicted {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
                age: age.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an entry is evicted due to running out of space"]
    pub struct PathSecretMapAddressEntryEvicted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
        #[doc = " Time since insertion of this entry"]
        pub age: core::time::Duration,
    }
    impl<'a> IntoEvent<api::PathSecretMapAddressEntryEvicted<'a>>
        for PathSecretMapAddressEntryEvicted<'a>
    {
        #[inline]
        fn into_event(self) -> api::PathSecretMapAddressEntryEvicted<'a> {
            let PathSecretMapAddressEntryEvicted {
                peer_address,
                credential_id,
                age,
            } = self;
            api::PathSecretMapAddressEntryEvicted {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
                age: age.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an UnknownPathSecret packet was sent"]
    pub struct UnknownPathSecretPacketSent<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::UnknownPathSecretPacketSent<'a>> for UnknownPathSecretPacketSent<'a> {
        #[inline]
        fn into_event(self) -> api::UnknownPathSecretPacketSent<'a> {
            let UnknownPathSecretPacketSent {
                peer_address,
                credential_id,
            } = self;
            api::UnknownPathSecretPacketSent {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an UnknownPathSecret packet was received"]
    pub struct UnknownPathSecretPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::UnknownPathSecretPacketReceived<'a>>
        for UnknownPathSecretPacketReceived<'a>
    {
        #[inline]
        fn into_event(self) -> api::UnknownPathSecretPacketReceived<'a> {
            let UnknownPathSecretPacketReceived {
                peer_address,
                credential_id,
            } = self;
            api::UnknownPathSecretPacketReceived {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an UnknownPathSecret packet was authentic and processed"]
    pub struct UnknownPathSecretPacketAccepted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::UnknownPathSecretPacketAccepted<'a>>
        for UnknownPathSecretPacketAccepted<'a>
    {
        #[inline]
        fn into_event(self) -> api::UnknownPathSecretPacketAccepted<'a> {
            let UnknownPathSecretPacketAccepted {
                peer_address,
                credential_id,
            } = self;
            api::UnknownPathSecretPacketAccepted {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an UnknownPathSecret packet was rejected as invalid"]
    pub struct UnknownPathSecretPacketRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::UnknownPathSecretPacketRejected<'a>>
        for UnknownPathSecretPacketRejected<'a>
    {
        #[inline]
        fn into_event(self) -> api::UnknownPathSecretPacketRejected<'a> {
            let UnknownPathSecretPacketRejected {
                peer_address,
                credential_id,
            } = self;
            api::UnknownPathSecretPacketRejected {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an UnknownPathSecret packet was dropped due to a missing entry"]
    pub struct UnknownPathSecretPacketDropped<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::UnknownPathSecretPacketDropped<'a>> for UnknownPathSecretPacketDropped<'a> {
        #[inline]
        fn into_event(self) -> api::UnknownPathSecretPacketDropped<'a> {
            let UnknownPathSecretPacketDropped {
                peer_address,
                credential_id,
            } = self;
            api::UnknownPathSecretPacketDropped {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when a credential is accepted (i.e., post packet authentication and passes replay"]
    #[doc = " check)."]
    pub struct KeyAccepted<'a> {
        pub credential_id: &'a [u8],
        pub key_id: u64,
        #[doc = " How far away this credential is from the leading edge of key IDs (after updating the edge)."]
        #[doc = ""]
        #[doc = " Zero if this shifted us forward."]
        pub gap: u64,
        #[doc = " How far away this credential is from the leading edge of key IDs (before updating the edge)."]
        #[doc = ""]
        #[doc = " Zero if this didn't change the leading edge."]
        pub forward_shift: u64,
    }
    impl<'a> IntoEvent<api::KeyAccepted<'a>> for KeyAccepted<'a> {
        #[inline]
        fn into_event(self) -> api::KeyAccepted<'a> {
            let KeyAccepted {
                credential_id,
                key_id,
                gap,
                forward_shift,
            } = self;
            api::KeyAccepted {
                credential_id: credential_id.into_event(),
                key_id: key_id.into_event(),
                gap: gap.into_event(),
                forward_shift: forward_shift.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when credential replay was definitely detected"]
    pub struct ReplayDefinitelyDetected<'a> {
        pub credential_id: &'a [u8],
        pub key_id: u64,
    }
    impl<'a> IntoEvent<api::ReplayDefinitelyDetected<'a>> for ReplayDefinitelyDetected<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayDefinitelyDetected<'a> {
            let ReplayDefinitelyDetected {
                credential_id,
                key_id,
            } = self;
            api::ReplayDefinitelyDetected {
                credential_id: credential_id.into_event(),
                key_id: key_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when credential replay was potentially detected, but could not be verified"]
    #[doc = " due to a limiting tracking window"]
    pub struct ReplayPotentiallyDetected<'a> {
        pub credential_id: &'a [u8],
        pub key_id: u64,
        pub gap: u64,
    }
    impl<'a> IntoEvent<api::ReplayPotentiallyDetected<'a>> for ReplayPotentiallyDetected<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayPotentiallyDetected<'a> {
            let ReplayPotentiallyDetected {
                credential_id,
                key_id,
                gap,
            } = self;
            api::ReplayPotentiallyDetected {
                credential_id: credential_id.into_event(),
                key_id: key_id.into_event(),
                gap: gap.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an ReplayDetected packet was sent"]
    pub struct ReplayDetectedPacketSent<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::ReplayDetectedPacketSent<'a>> for ReplayDetectedPacketSent<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayDetectedPacketSent<'a> {
            let ReplayDetectedPacketSent {
                peer_address,
                credential_id,
            } = self;
            api::ReplayDetectedPacketSent {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an ReplayDetected packet was received"]
    pub struct ReplayDetectedPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::ReplayDetectedPacketReceived<'a>> for ReplayDetectedPacketReceived<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayDetectedPacketReceived<'a> {
            let ReplayDetectedPacketReceived {
                peer_address,
                credential_id,
            } = self;
            api::ReplayDetectedPacketReceived {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an StaleKey packet was authentic and processed"]
    pub struct ReplayDetectedPacketAccepted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
        pub key_id: u64,
    }
    impl<'a> IntoEvent<api::ReplayDetectedPacketAccepted<'a>> for ReplayDetectedPacketAccepted<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayDetectedPacketAccepted<'a> {
            let ReplayDetectedPacketAccepted {
                peer_address,
                credential_id,
                key_id,
            } = self;
            api::ReplayDetectedPacketAccepted {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
                key_id: key_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an ReplayDetected packet was rejected as invalid"]
    pub struct ReplayDetectedPacketRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::ReplayDetectedPacketRejected<'a>> for ReplayDetectedPacketRejected<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayDetectedPacketRejected<'a> {
            let ReplayDetectedPacketRejected {
                peer_address,
                credential_id,
            } = self;
            api::ReplayDetectedPacketRejected {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an ReplayDetected packet was dropped due to a missing entry"]
    pub struct ReplayDetectedPacketDropped<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::ReplayDetectedPacketDropped<'a>> for ReplayDetectedPacketDropped<'a> {
        #[inline]
        fn into_event(self) -> api::ReplayDetectedPacketDropped<'a> {
            let ReplayDetectedPacketDropped {
                peer_address,
                credential_id,
            } = self;
            api::ReplayDetectedPacketDropped {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an StaleKey packet was sent"]
    pub struct StaleKeyPacketSent<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::StaleKeyPacketSent<'a>> for StaleKeyPacketSent<'a> {
        #[inline]
        fn into_event(self) -> api::StaleKeyPacketSent<'a> {
            let StaleKeyPacketSent {
                peer_address,
                credential_id,
            } = self;
            api::StaleKeyPacketSent {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an StaleKey packet was received"]
    pub struct StaleKeyPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::StaleKeyPacketReceived<'a>> for StaleKeyPacketReceived<'a> {
        #[inline]
        fn into_event(self) -> api::StaleKeyPacketReceived<'a> {
            let StaleKeyPacketReceived {
                peer_address,
                credential_id,
            } = self;
            api::StaleKeyPacketReceived {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an StaleKey packet was authentic and processed"]
    pub struct StaleKeyPacketAccepted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::StaleKeyPacketAccepted<'a>> for StaleKeyPacketAccepted<'a> {
        #[inline]
        fn into_event(self) -> api::StaleKeyPacketAccepted<'a> {
            let StaleKeyPacketAccepted {
                peer_address,
                credential_id,
            } = self;
            api::StaleKeyPacketAccepted {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an StaleKey packet was rejected as invalid"]
    pub struct StaleKeyPacketRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::StaleKeyPacketRejected<'a>> for StaleKeyPacketRejected<'a> {
        #[inline]
        fn into_event(self) -> api::StaleKeyPacketRejected<'a> {
            let StaleKeyPacketRejected {
                peer_address,
                credential_id,
            } = self;
            api::StaleKeyPacketRejected {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when an StaleKey packet was dropped due to a missing entry"]
    pub struct StaleKeyPacketDropped<'a> {
        pub peer_address: SocketAddress<'a>,
        pub credential_id: &'a [u8],
    }
    impl<'a> IntoEvent<api::StaleKeyPacketDropped<'a>> for StaleKeyPacketDropped<'a> {
        #[inline]
        fn into_event(self) -> api::StaleKeyPacketDropped<'a> {
            let StaleKeyPacketDropped {
                peer_address,
                credential_id,
            } = self;
            api::StaleKeyPacketDropped {
                peer_address: peer_address.into_event(),
                credential_id: credential_id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the cache is accessed by peer address"]
    #[doc = ""]
    #[doc = " This can be used to track cache hit ratios"]
    pub struct PathSecretMapAddressCacheAccessed<'a> {
        pub peer_address: SocketAddress<'a>,
        pub hit: bool,
    }
    impl<'a> IntoEvent<api::PathSecretMapAddressCacheAccessed<'a>>
        for PathSecretMapAddressCacheAccessed<'a>
    {
        #[inline]
        fn into_event(self) -> api::PathSecretMapAddressCacheAccessed<'a> {
            let PathSecretMapAddressCacheAccessed { peer_address, hit } = self;
            api::PathSecretMapAddressCacheAccessed {
                peer_address: peer_address.into_event(),
                hit: hit.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the cache is accessed by peer address successfully"]
    #[doc = ""]
    #[doc = " Provides more information about the accessed entry."]
    pub struct PathSecretMapAddressCacheAccessedHit<'a> {
        pub peer_address: SocketAddress<'a>,
        pub age: core::time::Duration,
    }
    impl<'a> IntoEvent<api::PathSecretMapAddressCacheAccessedHit<'a>>
        for PathSecretMapAddressCacheAccessedHit<'a>
    {
        #[inline]
        fn into_event(self) -> api::PathSecretMapAddressCacheAccessedHit<'a> {
            let PathSecretMapAddressCacheAccessedHit { peer_address, age } = self;
            api::PathSecretMapAddressCacheAccessedHit {
                peer_address: peer_address.into_event(),
                age: age.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the cache is accessed by path secret ID"]
    #[doc = ""]
    #[doc = " This can be used to track cache hit ratios"]
    pub struct PathSecretMapIdCacheAccessed<'a> {
        pub credential_id: &'a [u8],
        pub hit: bool,
    }
    impl<'a> IntoEvent<api::PathSecretMapIdCacheAccessed<'a>> for PathSecretMapIdCacheAccessed<'a> {
        #[inline]
        fn into_event(self) -> api::PathSecretMapIdCacheAccessed<'a> {
            let PathSecretMapIdCacheAccessed { credential_id, hit } = self;
            api::PathSecretMapIdCacheAccessed {
                credential_id: credential_id.into_event(),
                hit: hit.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the cache is accessed by path secret ID successfully"]
    #[doc = ""]
    #[doc = " Provides more information about the accessed entry."]
    pub struct PathSecretMapIdCacheAccessedHit<'a> {
        pub credential_id: &'a [u8],
        pub age: core::time::Duration,
    }
    impl<'a> IntoEvent<api::PathSecretMapIdCacheAccessedHit<'a>>
        for PathSecretMapIdCacheAccessedHit<'a>
    {
        #[inline]
        fn into_event(self) -> api::PathSecretMapIdCacheAccessedHit<'a> {
            let PathSecretMapIdCacheAccessedHit { credential_id, age } = self;
            api::PathSecretMapIdCacheAccessedHit {
                credential_id: credential_id.into_event(),
                age: age.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Emitted when the cleaner task performed a single cycle"]
    #[doc = ""]
    #[doc = " This can be used to track cache utilization"]
    pub struct PathSecretMapCleanerCycled {
        #[doc = " The number of Path Secret ID entries left after the cleaning cycle"]
        pub id_entries: usize,
        #[doc = " The number of Path Secret ID entries that were retired in the cycle"]
        pub id_entries_retired: usize,
        #[doc = " Count of entries accessed since the last cycle"]
        pub id_entries_active: usize,
        #[doc = " The utilization percentage of the active number of entries after the cycle"]
        pub id_entries_active_utilization: f32,
        #[doc = " The utilization percentage of the available number of entries after the cycle"]
        pub id_entries_utilization: f32,
        #[doc = " The utilization percentage of the available number of entries before the cycle"]
        pub id_entries_initial_utilization: f32,
        #[doc = " The number of SocketAddress entries left after the cleaning cycle"]
        pub address_entries: usize,
        #[doc = " Count of entries accessed since the last cycle"]
        pub address_entries_active: usize,
        #[doc = " The utilization percentage of the active number of entries after the cycle"]
        pub address_entries_active_utilization: f32,
        #[doc = " The number of SocketAddress entries that were retired in the cycle"]
        pub address_entries_retired: usize,
        #[doc = " The utilization percentage of the available number of address entries after the cycle"]
        pub address_entries_utilization: f32,
        #[doc = " The utilization percentage of the available number of address entries before the cycle"]
        pub address_entries_initial_utilization: f32,
        #[doc = " The number of handshake requests that are pending after the cleaning cycle"]
        pub handshake_requests: usize,
        #[doc = " The number of handshake requests that were retired in the cycle"]
        pub handshake_requests_retired: usize,
        #[doc = " How long we kept the handshake lock held (this blocks completing handshakes)."]
        pub handshake_lock_duration: core::time::Duration,
        #[doc = " Total duration of a cycle."]
        pub duration: core::time::Duration,
    }
    impl IntoEvent<api::PathSecretMapCleanerCycled> for PathSecretMapCleanerCycled {
        #[inline]
        fn into_event(self) -> api::PathSecretMapCleanerCycled {
            let PathSecretMapCleanerCycled {
                id_entries,
                id_entries_retired,
                id_entries_active,
                id_entries_active_utilization,
                id_entries_utilization,
                id_entries_initial_utilization,
                address_entries,
                address_entries_active,
                address_entries_active_utilization,
                address_entries_retired,
                address_entries_utilization,
                address_entries_initial_utilization,
                handshake_requests,
                handshake_requests_retired,
                handshake_lock_duration,
                duration,
            } = self;
            api::PathSecretMapCleanerCycled {
                id_entries: id_entries.into_event(),
                id_entries_retired: id_entries_retired.into_event(),
                id_entries_active: id_entries_active.into_event(),
                id_entries_active_utilization: id_entries_active_utilization.into_event(),
                id_entries_utilization: id_entries_utilization.into_event(),
                id_entries_initial_utilization: id_entries_initial_utilization.into_event(),
                address_entries: address_entries.into_event(),
                address_entries_active: address_entries_active.into_event(),
                address_entries_active_utilization: address_entries_active_utilization.into_event(),
                address_entries_retired: address_entries_retired.into_event(),
                address_entries_utilization: address_entries_utilization.into_event(),
                address_entries_initial_utilization: address_entries_initial_utilization
                    .into_event(),
                handshake_requests: handshake_requests.into_event(),
                handshake_requests_retired: handshake_requests_retired.into_event(),
                handshake_lock_duration: handshake_lock_duration.into_event(),
                duration: duration.into_event(),
            }
        }
    }
}
pub use traits::*;
mod traits {
    use super::*;
    use crate::event::Meta;
    use core::fmt;
    use s2n_quic_core::query;
    #[doc = r" Allows for events to be subscribed to"]
    pub trait Subscriber: 'static + Send + Sync {
        #[doc = r" An application provided type associated with each connection."]
        #[doc = r""]
        #[doc = r" The context provides a mechanism for applications to provide a custom type"]
        #[doc = r" and update it on each event, e.g. computing statistics. Each event"]
        #[doc = r" invocation (e.g. [`Subscriber::on_packet_sent`]) also provides mutable"]
        #[doc = r" access to the context `&mut ConnectionContext` and allows for updating the"]
        #[doc = r" context."]
        #[doc = r""]
        #[doc = r" ```no_run"]
        #[doc = r" # mod s2n_quic { pub mod provider { pub mod event {"]
        #[doc = r" #     pub use s2n_quic_core::event::{api as events, api::ConnectionInfo, api::ConnectionMeta, Subscriber};"]
        #[doc = r" # }}}"]
        #[doc = r" use s2n_quic::provider::event::{"]
        #[doc = r"     ConnectionInfo, ConnectionMeta, Subscriber, events::PacketSent"]
        #[doc = r" };"]
        #[doc = r""]
        #[doc = r" pub struct MyEventSubscriber;"]
        #[doc = r""]
        #[doc = r" pub struct MyEventContext {"]
        #[doc = r"     packet_sent: u64,"]
        #[doc = r" }"]
        #[doc = r""]
        #[doc = r" impl Subscriber for MyEventSubscriber {"]
        #[doc = r"     type ConnectionContext = MyEventContext;"]
        #[doc = r""]
        #[doc = r"     fn create_connection_context("]
        #[doc = r"         &mut self, _meta: &ConnectionMeta,"]
        #[doc = r"         _info: &ConnectionInfo,"]
        #[doc = r"     ) -> Self::ConnectionContext {"]
        #[doc = r"         MyEventContext { packet_sent: 0 }"]
        #[doc = r"     }"]
        #[doc = r""]
        #[doc = r"     fn on_packet_sent("]
        #[doc = r"         &mut self,"]
        #[doc = r"         context: &mut Self::ConnectionContext,"]
        #[doc = r"         _meta: &ConnectionMeta,"]
        #[doc = r"         _event: &PacketSent,"]
        #[doc = r"     ) {"]
        #[doc = r"         context.packet_sent += 1;"]
        #[doc = r"     }"]
        #[doc = r" }"]
        #[doc = r"  ```"]
        type ConnectionContext: 'static + Send + Sync;
        #[doc = r" Creates a context to be passed to each connection-related event"]
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext;
        #[doc = "Called when the `AcceptorTcpStarted` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStarted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpLoopIterationCompleted` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpLoopIterationCompleted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpFreshEnqueued` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_fresh_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshEnqueued,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpFreshBatchCompleted` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshBatchCompleted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpStreamDropped` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_stream_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpStreamReplaced` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_stream_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamReplaced,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpPacketReceived` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpPacketDropped` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpStreamEnqueued` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamEnqueued,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorTcpIoError` event is triggered"]
        #[inline]
        fn on_acceptor_tcp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpIoError,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorUdpStarted` event is triggered"]
        #[inline]
        fn on_acceptor_udp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStarted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorUdpDatagramReceived` event is triggered"]
        #[inline]
        fn on_acceptor_udp_datagram_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpDatagramReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorUdpPacketReceived` event is triggered"]
        #[inline]
        fn on_acceptor_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorUdpPacketDropped` event is triggered"]
        #[inline]
        fn on_acceptor_udp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorUdpStreamEnqueued` event is triggered"]
        #[inline]
        fn on_acceptor_udp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStreamEnqueued,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorUdpIoError` event is triggered"]
        #[inline]
        fn on_acceptor_udp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpIoError,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorStreamPruned` event is triggered"]
        #[inline]
        fn on_acceptor_stream_pruned(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamPruned,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `AcceptorStreamDequeued` event is triggered"]
        #[inline]
        fn on_acceptor_stream_dequeued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamDequeued,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteFlushed` event is triggered"]
        #[inline]
        fn on_stream_write_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFlushed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteFinFlushed` event is triggered"]
        #[inline]
        fn on_stream_write_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFinFlushed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteBlocked` event is triggered"]
        #[inline]
        fn on_stream_write_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteBlocked,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteErrored` event is triggered"]
        #[inline]
        fn on_stream_write_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteErrored,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteKeyUpdated` event is triggered"]
        #[inline]
        fn on_stream_write_key_updated(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteKeyUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteShutdown` event is triggered"]
        #[inline]
        fn on_stream_write_shutdown(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteShutdown,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteSocketFlushed` event is triggered"]
        #[inline]
        fn on_stream_write_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketFlushed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteSocketBlocked` event is triggered"]
        #[inline]
        fn on_stream_write_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketBlocked,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamWriteSocketErrored` event is triggered"]
        #[inline]
        fn on_stream_write_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketErrored,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadFlushed` event is triggered"]
        #[inline]
        fn on_stream_read_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFlushed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadFinFlushed` event is triggered"]
        #[inline]
        fn on_stream_read_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFinFlushed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadBlocked` event is triggered"]
        #[inline]
        fn on_stream_read_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadBlocked,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadErrored` event is triggered"]
        #[inline]
        fn on_stream_read_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadErrored,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadKeyUpdated` event is triggered"]
        #[inline]
        fn on_stream_read_key_updated(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadKeyUpdated,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadShutdown` event is triggered"]
        #[inline]
        fn on_stream_read_shutdown(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadShutdown,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadSocketFlushed` event is triggered"]
        #[inline]
        fn on_stream_read_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketFlushed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadSocketBlocked` event is triggered"]
        #[inline]
        fn on_stream_read_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketBlocked,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamReadSocketErrored` event is triggered"]
        #[inline]
        fn on_stream_read_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketErrored,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamTcpConnect` event is triggered"]
        #[inline]
        fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamConnect` event is triggered"]
        #[inline]
        fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StreamConnectError` event is triggered"]
        #[inline]
        fn on_stream_connect_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StreamConnectError,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ConnectionClosed` event is triggered"]
        #[inline]
        fn on_connection_closed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointInitialized` event is triggered"]
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapInitialized` event is triggered"]
        #[inline]
        fn on_path_secret_map_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapInitialized,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapUninitialized` event is triggered"]
        #[inline]
        fn on_path_secret_map_uninitialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapUninitialized,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapBackgroundHandshakeRequested` event is triggered"]
        #[inline]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapBackgroundHandshakeRequested,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapEntryInserted` event is triggered"]
        #[inline]
        fn on_path_secret_map_entry_inserted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryInserted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapEntryReady` event is triggered"]
        #[inline]
        fn on_path_secret_map_entry_ready(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReady,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapEntryReplaced` event is triggered"]
        #[inline]
        fn on_path_secret_map_entry_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReplaced,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapIdEntryEvicted` event is triggered"]
        #[inline]
        fn on_path_secret_map_id_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdEntryEvicted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapAddressEntryEvicted` event is triggered"]
        #[inline]
        fn on_path_secret_map_address_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressEntryEvicted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `UnknownPathSecretPacketSent` event is triggered"]
        #[inline]
        fn on_unknown_path_secret_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketSent,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `UnknownPathSecretPacketReceived` event is triggered"]
        #[inline]
        fn on_unknown_path_secret_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `UnknownPathSecretPacketAccepted` event is triggered"]
        #[inline]
        fn on_unknown_path_secret_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketAccepted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `UnknownPathSecretPacketRejected` event is triggered"]
        #[inline]
        fn on_unknown_path_secret_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketRejected,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `UnknownPathSecretPacketDropped` event is triggered"]
        #[inline]
        fn on_unknown_path_secret_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `KeyAccepted` event is triggered"]
        #[inline]
        fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayDefinitelyDetected` event is triggered"]
        #[inline]
        fn on_replay_definitely_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDefinitelyDetected,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayPotentiallyDetected` event is triggered"]
        #[inline]
        fn on_replay_potentially_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayPotentiallyDetected,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayDetectedPacketSent` event is triggered"]
        #[inline]
        fn on_replay_detected_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketSent,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayDetectedPacketReceived` event is triggered"]
        #[inline]
        fn on_replay_detected_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayDetectedPacketAccepted` event is triggered"]
        #[inline]
        fn on_replay_detected_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketAccepted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayDetectedPacketRejected` event is triggered"]
        #[inline]
        fn on_replay_detected_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketRejected,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ReplayDetectedPacketDropped` event is triggered"]
        #[inline]
        fn on_replay_detected_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StaleKeyPacketSent` event is triggered"]
        #[inline]
        fn on_stale_key_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketSent,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StaleKeyPacketReceived` event is triggered"]
        #[inline]
        fn on_stale_key_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StaleKeyPacketAccepted` event is triggered"]
        #[inline]
        fn on_stale_key_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketAccepted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StaleKeyPacketRejected` event is triggered"]
        #[inline]
        fn on_stale_key_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketRejected,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `StaleKeyPacketDropped` event is triggered"]
        #[inline]
        fn on_stale_key_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketDropped,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapAddressCacheAccessed` event is triggered"]
        #[inline]
        fn on_path_secret_map_address_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessed,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapAddressCacheAccessedHit` event is triggered"]
        #[inline]
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessedHit,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapIdCacheAccessed` event is triggered"]
        #[inline]
        fn on_path_secret_map_id_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessed,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapIdCacheAccessedHit` event is triggered"]
        #[inline]
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessedHit,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapCleanerCycled` event is triggered"]
        #[inline]
        fn on_path_secret_map_cleaner_cycled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapCleanerCycled,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to the endpoint and all connections"]
        #[inline]
        fn on_event<M: Meta, E: Event>(&self, meta: &M, event: &E) {
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Called for each event that relates to a connection"]
        #[inline]
        fn on_connection_event<E: Event>(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = r" Used for querying the `Subscriber::ConnectionContext` on a Subscriber"]
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query.execute(context)
        }
    }
    impl<T: Subscriber> Subscriber for std::sync::Arc<T> {
        type ConnectionContext = T::ConnectionContext;
        #[inline]
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            self.as_ref().create_connection_context(meta, info)
        }
        #[inline]
        fn on_acceptor_tcp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStarted,
        ) {
            self.as_ref().on_acceptor_tcp_started(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpLoopIterationCompleted,
        ) {
            self.as_ref()
                .on_acceptor_tcp_loop_iteration_completed(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_fresh_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshEnqueued,
        ) {
            self.as_ref().on_acceptor_tcp_fresh_enqueued(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshBatchCompleted,
        ) {
            self.as_ref()
                .on_acceptor_tcp_fresh_batch_completed(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamDropped,
        ) {
            self.as_ref().on_acceptor_tcp_stream_dropped(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamReplaced,
        ) {
            self.as_ref().on_acceptor_tcp_stream_replaced(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketReceived,
        ) {
            self.as_ref().on_acceptor_tcp_packet_received(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketDropped,
        ) {
            self.as_ref().on_acceptor_tcp_packet_dropped(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamEnqueued,
        ) {
            self.as_ref().on_acceptor_tcp_stream_enqueued(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpIoError,
        ) {
            self.as_ref().on_acceptor_tcp_io_error(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStarted,
        ) {
            self.as_ref().on_acceptor_udp_started(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_datagram_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpDatagramReceived,
        ) {
            self.as_ref().on_acceptor_udp_datagram_received(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketReceived,
        ) {
            self.as_ref().on_acceptor_udp_packet_received(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketDropped,
        ) {
            self.as_ref().on_acceptor_udp_packet_dropped(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStreamEnqueued,
        ) {
            self.as_ref().on_acceptor_udp_stream_enqueued(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpIoError,
        ) {
            self.as_ref().on_acceptor_udp_io_error(meta, event);
        }
        #[inline]
        fn on_acceptor_stream_pruned(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamPruned,
        ) {
            self.as_ref().on_acceptor_stream_pruned(meta, event);
        }
        #[inline]
        fn on_acceptor_stream_dequeued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamDequeued,
        ) {
            self.as_ref().on_acceptor_stream_dequeued(meta, event);
        }
        #[inline]
        fn on_stream_write_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFlushed,
        ) {
            self.as_ref().on_stream_write_flushed(context, meta, event);
        }
        #[inline]
        fn on_stream_write_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFinFlushed,
        ) {
            self.as_ref()
                .on_stream_write_fin_flushed(context, meta, event);
        }
        #[inline]
        fn on_stream_write_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteBlocked,
        ) {
            self.as_ref().on_stream_write_blocked(context, meta, event);
        }
        #[inline]
        fn on_stream_write_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteErrored,
        ) {
            self.as_ref().on_stream_write_errored(context, meta, event);
        }
        #[inline]
        fn on_stream_write_key_updated(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteKeyUpdated,
        ) {
            self.as_ref()
                .on_stream_write_key_updated(context, meta, event);
        }
        #[inline]
        fn on_stream_write_shutdown(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteShutdown,
        ) {
            self.as_ref().on_stream_write_shutdown(context, meta, event);
        }
        #[inline]
        fn on_stream_write_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketFlushed,
        ) {
            self.as_ref()
                .on_stream_write_socket_flushed(context, meta, event);
        }
        #[inline]
        fn on_stream_write_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketBlocked,
        ) {
            self.as_ref()
                .on_stream_write_socket_blocked(context, meta, event);
        }
        #[inline]
        fn on_stream_write_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketErrored,
        ) {
            self.as_ref()
                .on_stream_write_socket_errored(context, meta, event);
        }
        #[inline]
        fn on_stream_read_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFlushed,
        ) {
            self.as_ref().on_stream_read_flushed(context, meta, event);
        }
        #[inline]
        fn on_stream_read_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFinFlushed,
        ) {
            self.as_ref()
                .on_stream_read_fin_flushed(context, meta, event);
        }
        #[inline]
        fn on_stream_read_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadBlocked,
        ) {
            self.as_ref().on_stream_read_blocked(context, meta, event);
        }
        #[inline]
        fn on_stream_read_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadErrored,
        ) {
            self.as_ref().on_stream_read_errored(context, meta, event);
        }
        #[inline]
        fn on_stream_read_key_updated(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadKeyUpdated,
        ) {
            self.as_ref()
                .on_stream_read_key_updated(context, meta, event);
        }
        #[inline]
        fn on_stream_read_shutdown(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadShutdown,
        ) {
            self.as_ref().on_stream_read_shutdown(context, meta, event);
        }
        #[inline]
        fn on_stream_read_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketFlushed,
        ) {
            self.as_ref()
                .on_stream_read_socket_flushed(context, meta, event);
        }
        #[inline]
        fn on_stream_read_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketBlocked,
        ) {
            self.as_ref()
                .on_stream_read_socket_blocked(context, meta, event);
        }
        #[inline]
        fn on_stream_read_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketErrored,
        ) {
            self.as_ref()
                .on_stream_read_socket_errored(context, meta, event);
        }
        #[inline]
        fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
            self.as_ref().on_stream_tcp_connect(meta, event);
        }
        #[inline]
        fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
            self.as_ref().on_stream_connect(meta, event);
        }
        #[inline]
        fn on_stream_connect_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StreamConnectError,
        ) {
            self.as_ref().on_stream_connect_error(meta, event);
        }
        #[inline]
        fn on_connection_closed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            self.as_ref().on_connection_closed(context, meta, event);
        }
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            self.as_ref().on_endpoint_initialized(meta, event);
        }
        #[inline]
        fn on_path_secret_map_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapInitialized,
        ) {
            self.as_ref().on_path_secret_map_initialized(meta, event);
        }
        #[inline]
        fn on_path_secret_map_uninitialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapUninitialized,
        ) {
            self.as_ref().on_path_secret_map_uninitialized(meta, event);
        }
        #[inline]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapBackgroundHandshakeRequested,
        ) {
            self.as_ref()
                .on_path_secret_map_background_handshake_requested(meta, event);
        }
        #[inline]
        fn on_path_secret_map_entry_inserted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryInserted,
        ) {
            self.as_ref().on_path_secret_map_entry_inserted(meta, event);
        }
        #[inline]
        fn on_path_secret_map_entry_ready(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReady,
        ) {
            self.as_ref().on_path_secret_map_entry_ready(meta, event);
        }
        #[inline]
        fn on_path_secret_map_entry_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReplaced,
        ) {
            self.as_ref().on_path_secret_map_entry_replaced(meta, event);
        }
        #[inline]
        fn on_path_secret_map_id_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdEntryEvicted,
        ) {
            self.as_ref()
                .on_path_secret_map_id_entry_evicted(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressEntryEvicted,
        ) {
            self.as_ref()
                .on_path_secret_map_address_entry_evicted(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketSent,
        ) {
            self.as_ref()
                .on_unknown_path_secret_packet_sent(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketReceived,
        ) {
            self.as_ref()
                .on_unknown_path_secret_packet_received(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketAccepted,
        ) {
            self.as_ref()
                .on_unknown_path_secret_packet_accepted(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketRejected,
        ) {
            self.as_ref()
                .on_unknown_path_secret_packet_rejected(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketDropped,
        ) {
            self.as_ref()
                .on_unknown_path_secret_packet_dropped(meta, event);
        }
        #[inline]
        fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
            self.as_ref().on_key_accepted(meta, event);
        }
        #[inline]
        fn on_replay_definitely_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDefinitelyDetected,
        ) {
            self.as_ref().on_replay_definitely_detected(meta, event);
        }
        #[inline]
        fn on_replay_potentially_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayPotentiallyDetected,
        ) {
            self.as_ref().on_replay_potentially_detected(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketSent,
        ) {
            self.as_ref().on_replay_detected_packet_sent(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketReceived,
        ) {
            self.as_ref()
                .on_replay_detected_packet_received(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketAccepted,
        ) {
            self.as_ref()
                .on_replay_detected_packet_accepted(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketRejected,
        ) {
            self.as_ref()
                .on_replay_detected_packet_rejected(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketDropped,
        ) {
            self.as_ref().on_replay_detected_packet_dropped(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketSent,
        ) {
            self.as_ref().on_stale_key_packet_sent(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketReceived,
        ) {
            self.as_ref().on_stale_key_packet_received(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketAccepted,
        ) {
            self.as_ref().on_stale_key_packet_accepted(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketRejected,
        ) {
            self.as_ref().on_stale_key_packet_rejected(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketDropped,
        ) {
            self.as_ref().on_stale_key_packet_dropped(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessed,
        ) {
            self.as_ref()
                .on_path_secret_map_address_cache_accessed(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessedHit,
        ) {
            self.as_ref()
                .on_path_secret_map_address_cache_accessed_hit(meta, event);
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessed,
        ) {
            self.as_ref()
                .on_path_secret_map_id_cache_accessed(meta, event);
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessedHit,
        ) {
            self.as_ref()
                .on_path_secret_map_id_cache_accessed_hit(meta, event);
        }
        #[inline]
        fn on_path_secret_map_cleaner_cycled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapCleanerCycled,
        ) {
            self.as_ref().on_path_secret_map_cleaner_cycled(meta, event);
        }
        #[inline]
        fn on_event<M: Meta, E: Event>(&self, meta: &M, event: &E) {
            self.as_ref().on_event(meta, event);
        }
        #[inline]
        fn on_connection_event<E: Event>(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            self.as_ref().on_connection_event(context, meta, event);
        }
    }
    #[doc = r" Subscriber is implemented for a 2-element tuple to make it easy to compose multiple"]
    #[doc = r" subscribers."]
    impl<A, B> Subscriber for (A, B)
    where
        A: Subscriber,
        B: Subscriber,
    {
        type ConnectionContext = (A::ConnectionContext, B::ConnectionContext);
        #[inline]
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            (
                self.0.create_connection_context(meta, info),
                self.1.create_connection_context(meta, info),
            )
        }
        #[inline]
        fn on_acceptor_tcp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStarted,
        ) {
            (self.0).on_acceptor_tcp_started(meta, event);
            (self.1).on_acceptor_tcp_started(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpLoopIterationCompleted,
        ) {
            (self.0).on_acceptor_tcp_loop_iteration_completed(meta, event);
            (self.1).on_acceptor_tcp_loop_iteration_completed(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_fresh_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshEnqueued,
        ) {
            (self.0).on_acceptor_tcp_fresh_enqueued(meta, event);
            (self.1).on_acceptor_tcp_fresh_enqueued(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshBatchCompleted,
        ) {
            (self.0).on_acceptor_tcp_fresh_batch_completed(meta, event);
            (self.1).on_acceptor_tcp_fresh_batch_completed(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamDropped,
        ) {
            (self.0).on_acceptor_tcp_stream_dropped(meta, event);
            (self.1).on_acceptor_tcp_stream_dropped(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamReplaced,
        ) {
            (self.0).on_acceptor_tcp_stream_replaced(meta, event);
            (self.1).on_acceptor_tcp_stream_replaced(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketReceived,
        ) {
            (self.0).on_acceptor_tcp_packet_received(meta, event);
            (self.1).on_acceptor_tcp_packet_received(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketDropped,
        ) {
            (self.0).on_acceptor_tcp_packet_dropped(meta, event);
            (self.1).on_acceptor_tcp_packet_dropped(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamEnqueued,
        ) {
            (self.0).on_acceptor_tcp_stream_enqueued(meta, event);
            (self.1).on_acceptor_tcp_stream_enqueued(meta, event);
        }
        #[inline]
        fn on_acceptor_tcp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpIoError,
        ) {
            (self.0).on_acceptor_tcp_io_error(meta, event);
            (self.1).on_acceptor_tcp_io_error(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStarted,
        ) {
            (self.0).on_acceptor_udp_started(meta, event);
            (self.1).on_acceptor_udp_started(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_datagram_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpDatagramReceived,
        ) {
            (self.0).on_acceptor_udp_datagram_received(meta, event);
            (self.1).on_acceptor_udp_datagram_received(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketReceived,
        ) {
            (self.0).on_acceptor_udp_packet_received(meta, event);
            (self.1).on_acceptor_udp_packet_received(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketDropped,
        ) {
            (self.0).on_acceptor_udp_packet_dropped(meta, event);
            (self.1).on_acceptor_udp_packet_dropped(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStreamEnqueued,
        ) {
            (self.0).on_acceptor_udp_stream_enqueued(meta, event);
            (self.1).on_acceptor_udp_stream_enqueued(meta, event);
        }
        #[inline]
        fn on_acceptor_udp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpIoError,
        ) {
            (self.0).on_acceptor_udp_io_error(meta, event);
            (self.1).on_acceptor_udp_io_error(meta, event);
        }
        #[inline]
        fn on_acceptor_stream_pruned(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamPruned,
        ) {
            (self.0).on_acceptor_stream_pruned(meta, event);
            (self.1).on_acceptor_stream_pruned(meta, event);
        }
        #[inline]
        fn on_acceptor_stream_dequeued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamDequeued,
        ) {
            (self.0).on_acceptor_stream_dequeued(meta, event);
            (self.1).on_acceptor_stream_dequeued(meta, event);
        }
        #[inline]
        fn on_stream_write_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFlushed,
        ) {
            (self.0).on_stream_write_flushed(&context.0, meta, event);
            (self.1).on_stream_write_flushed(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFinFlushed,
        ) {
            (self.0).on_stream_write_fin_flushed(&context.0, meta, event);
            (self.1).on_stream_write_fin_flushed(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteBlocked,
        ) {
            (self.0).on_stream_write_blocked(&context.0, meta, event);
            (self.1).on_stream_write_blocked(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteErrored,
        ) {
            (self.0).on_stream_write_errored(&context.0, meta, event);
            (self.1).on_stream_write_errored(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_key_updated(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteKeyUpdated,
        ) {
            (self.0).on_stream_write_key_updated(&context.0, meta, event);
            (self.1).on_stream_write_key_updated(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_shutdown(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteShutdown,
        ) {
            (self.0).on_stream_write_shutdown(&context.0, meta, event);
            (self.1).on_stream_write_shutdown(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketFlushed,
        ) {
            (self.0).on_stream_write_socket_flushed(&context.0, meta, event);
            (self.1).on_stream_write_socket_flushed(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketBlocked,
        ) {
            (self.0).on_stream_write_socket_blocked(&context.0, meta, event);
            (self.1).on_stream_write_socket_blocked(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_write_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketErrored,
        ) {
            (self.0).on_stream_write_socket_errored(&context.0, meta, event);
            (self.1).on_stream_write_socket_errored(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFlushed,
        ) {
            (self.0).on_stream_read_flushed(&context.0, meta, event);
            (self.1).on_stream_read_flushed(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_fin_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFinFlushed,
        ) {
            (self.0).on_stream_read_fin_flushed(&context.0, meta, event);
            (self.1).on_stream_read_fin_flushed(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadBlocked,
        ) {
            (self.0).on_stream_read_blocked(&context.0, meta, event);
            (self.1).on_stream_read_blocked(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadErrored,
        ) {
            (self.0).on_stream_read_errored(&context.0, meta, event);
            (self.1).on_stream_read_errored(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_key_updated(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadKeyUpdated,
        ) {
            (self.0).on_stream_read_key_updated(&context.0, meta, event);
            (self.1).on_stream_read_key_updated(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_shutdown(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadShutdown,
        ) {
            (self.0).on_stream_read_shutdown(&context.0, meta, event);
            (self.1).on_stream_read_shutdown(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_socket_flushed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketFlushed,
        ) {
            (self.0).on_stream_read_socket_flushed(&context.0, meta, event);
            (self.1).on_stream_read_socket_flushed(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_socket_blocked(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketBlocked,
        ) {
            (self.0).on_stream_read_socket_blocked(&context.0, meta, event);
            (self.1).on_stream_read_socket_blocked(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_read_socket_errored(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketErrored,
        ) {
            (self.0).on_stream_read_socket_errored(&context.0, meta, event);
            (self.1).on_stream_read_socket_errored(&context.1, meta, event);
        }
        #[inline]
        fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
            (self.0).on_stream_tcp_connect(meta, event);
            (self.1).on_stream_tcp_connect(meta, event);
        }
        #[inline]
        fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
            (self.0).on_stream_connect(meta, event);
            (self.1).on_stream_connect(meta, event);
        }
        #[inline]
        fn on_stream_connect_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StreamConnectError,
        ) {
            (self.0).on_stream_connect_error(meta, event);
            (self.1).on_stream_connect_error(meta, event);
        }
        #[inline]
        fn on_connection_closed(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            (self.0).on_connection_closed(&context.0, meta, event);
            (self.1).on_connection_closed(&context.1, meta, event);
        }
        #[inline]
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            (self.0).on_endpoint_initialized(meta, event);
            (self.1).on_endpoint_initialized(meta, event);
        }
        #[inline]
        fn on_path_secret_map_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapInitialized,
        ) {
            (self.0).on_path_secret_map_initialized(meta, event);
            (self.1).on_path_secret_map_initialized(meta, event);
        }
        #[inline]
        fn on_path_secret_map_uninitialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapUninitialized,
        ) {
            (self.0).on_path_secret_map_uninitialized(meta, event);
            (self.1).on_path_secret_map_uninitialized(meta, event);
        }
        #[inline]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapBackgroundHandshakeRequested,
        ) {
            (self.0).on_path_secret_map_background_handshake_requested(meta, event);
            (self.1).on_path_secret_map_background_handshake_requested(meta, event);
        }
        #[inline]
        fn on_path_secret_map_entry_inserted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryInserted,
        ) {
            (self.0).on_path_secret_map_entry_inserted(meta, event);
            (self.1).on_path_secret_map_entry_inserted(meta, event);
        }
        #[inline]
        fn on_path_secret_map_entry_ready(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReady,
        ) {
            (self.0).on_path_secret_map_entry_ready(meta, event);
            (self.1).on_path_secret_map_entry_ready(meta, event);
        }
        #[inline]
        fn on_path_secret_map_entry_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReplaced,
        ) {
            (self.0).on_path_secret_map_entry_replaced(meta, event);
            (self.1).on_path_secret_map_entry_replaced(meta, event);
        }
        #[inline]
        fn on_path_secret_map_id_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdEntryEvicted,
        ) {
            (self.0).on_path_secret_map_id_entry_evicted(meta, event);
            (self.1).on_path_secret_map_id_entry_evicted(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressEntryEvicted,
        ) {
            (self.0).on_path_secret_map_address_entry_evicted(meta, event);
            (self.1).on_path_secret_map_address_entry_evicted(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketSent,
        ) {
            (self.0).on_unknown_path_secret_packet_sent(meta, event);
            (self.1).on_unknown_path_secret_packet_sent(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketReceived,
        ) {
            (self.0).on_unknown_path_secret_packet_received(meta, event);
            (self.1).on_unknown_path_secret_packet_received(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketAccepted,
        ) {
            (self.0).on_unknown_path_secret_packet_accepted(meta, event);
            (self.1).on_unknown_path_secret_packet_accepted(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketRejected,
        ) {
            (self.0).on_unknown_path_secret_packet_rejected(meta, event);
            (self.1).on_unknown_path_secret_packet_rejected(meta, event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketDropped,
        ) {
            (self.0).on_unknown_path_secret_packet_dropped(meta, event);
            (self.1).on_unknown_path_secret_packet_dropped(meta, event);
        }
        #[inline]
        fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
            (self.0).on_key_accepted(meta, event);
            (self.1).on_key_accepted(meta, event);
        }
        #[inline]
        fn on_replay_definitely_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDefinitelyDetected,
        ) {
            (self.0).on_replay_definitely_detected(meta, event);
            (self.1).on_replay_definitely_detected(meta, event);
        }
        #[inline]
        fn on_replay_potentially_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayPotentiallyDetected,
        ) {
            (self.0).on_replay_potentially_detected(meta, event);
            (self.1).on_replay_potentially_detected(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketSent,
        ) {
            (self.0).on_replay_detected_packet_sent(meta, event);
            (self.1).on_replay_detected_packet_sent(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketReceived,
        ) {
            (self.0).on_replay_detected_packet_received(meta, event);
            (self.1).on_replay_detected_packet_received(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketAccepted,
        ) {
            (self.0).on_replay_detected_packet_accepted(meta, event);
            (self.1).on_replay_detected_packet_accepted(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketRejected,
        ) {
            (self.0).on_replay_detected_packet_rejected(meta, event);
            (self.1).on_replay_detected_packet_rejected(meta, event);
        }
        #[inline]
        fn on_replay_detected_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketDropped,
        ) {
            (self.0).on_replay_detected_packet_dropped(meta, event);
            (self.1).on_replay_detected_packet_dropped(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketSent,
        ) {
            (self.0).on_stale_key_packet_sent(meta, event);
            (self.1).on_stale_key_packet_sent(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketReceived,
        ) {
            (self.0).on_stale_key_packet_received(meta, event);
            (self.1).on_stale_key_packet_received(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketAccepted,
        ) {
            (self.0).on_stale_key_packet_accepted(meta, event);
            (self.1).on_stale_key_packet_accepted(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketRejected,
        ) {
            (self.0).on_stale_key_packet_rejected(meta, event);
            (self.1).on_stale_key_packet_rejected(meta, event);
        }
        #[inline]
        fn on_stale_key_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketDropped,
        ) {
            (self.0).on_stale_key_packet_dropped(meta, event);
            (self.1).on_stale_key_packet_dropped(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessed,
        ) {
            (self.0).on_path_secret_map_address_cache_accessed(meta, event);
            (self.1).on_path_secret_map_address_cache_accessed(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessedHit,
        ) {
            (self.0).on_path_secret_map_address_cache_accessed_hit(meta, event);
            (self.1).on_path_secret_map_address_cache_accessed_hit(meta, event);
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessed,
        ) {
            (self.0).on_path_secret_map_id_cache_accessed(meta, event);
            (self.1).on_path_secret_map_id_cache_accessed(meta, event);
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessedHit,
        ) {
            (self.0).on_path_secret_map_id_cache_accessed_hit(meta, event);
            (self.1).on_path_secret_map_id_cache_accessed_hit(meta, event);
        }
        #[inline]
        fn on_path_secret_map_cleaner_cycled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapCleanerCycled,
        ) {
            (self.0).on_path_secret_map_cleaner_cycled(meta, event);
            (self.1).on_path_secret_map_cleaner_cycled(meta, event);
        }
        #[inline]
        fn on_event<M: Meta, E: Event>(&self, meta: &M, event: &E) {
            self.0.on_event(meta, event);
            self.1.on_event(meta, event);
        }
        #[inline]
        fn on_connection_event<E: Event>(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &E,
        ) {
            self.0.on_connection_event(&context.0, meta, event);
            self.1.on_connection_event(&context.1, meta, event);
        }
        #[inline]
        fn query(
            context: &Self::ConnectionContext,
            query: &mut dyn query::Query,
        ) -> query::ControlFlow {
            query
                .execute(context)
                .and_then(|| A::query(&context.0, query))
                .and_then(|| B::query(&context.1, query))
        }
    }
    pub trait EndpointPublisher {
        #[doc = "Publishes a `AcceptorTcpStarted` event to the publisher's subscriber"]
        fn on_acceptor_tcp_started(&self, event: builder::AcceptorTcpStarted);
        #[doc = "Publishes a `AcceptorTcpLoopIterationCompleted` event to the publisher's subscriber"]
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            event: builder::AcceptorTcpLoopIterationCompleted,
        );
        #[doc = "Publishes a `AcceptorTcpFreshEnqueued` event to the publisher's subscriber"]
        fn on_acceptor_tcp_fresh_enqueued(&self, event: builder::AcceptorTcpFreshEnqueued);
        #[doc = "Publishes a `AcceptorTcpFreshBatchCompleted` event to the publisher's subscriber"]
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            event: builder::AcceptorTcpFreshBatchCompleted,
        );
        #[doc = "Publishes a `AcceptorTcpStreamDropped` event to the publisher's subscriber"]
        fn on_acceptor_tcp_stream_dropped(&self, event: builder::AcceptorTcpStreamDropped);
        #[doc = "Publishes a `AcceptorTcpStreamReplaced` event to the publisher's subscriber"]
        fn on_acceptor_tcp_stream_replaced(&self, event: builder::AcceptorTcpStreamReplaced);
        #[doc = "Publishes a `AcceptorTcpPacketReceived` event to the publisher's subscriber"]
        fn on_acceptor_tcp_packet_received(&self, event: builder::AcceptorTcpPacketReceived);
        #[doc = "Publishes a `AcceptorTcpPacketDropped` event to the publisher's subscriber"]
        fn on_acceptor_tcp_packet_dropped(&self, event: builder::AcceptorTcpPacketDropped);
        #[doc = "Publishes a `AcceptorTcpStreamEnqueued` event to the publisher's subscriber"]
        fn on_acceptor_tcp_stream_enqueued(&self, event: builder::AcceptorTcpStreamEnqueued);
        #[doc = "Publishes a `AcceptorTcpIoError` event to the publisher's subscriber"]
        fn on_acceptor_tcp_io_error(&self, event: builder::AcceptorTcpIoError);
        #[doc = "Publishes a `AcceptorUdpStarted` event to the publisher's subscriber"]
        fn on_acceptor_udp_started(&self, event: builder::AcceptorUdpStarted);
        #[doc = "Publishes a `AcceptorUdpDatagramReceived` event to the publisher's subscriber"]
        fn on_acceptor_udp_datagram_received(&self, event: builder::AcceptorUdpDatagramReceived);
        #[doc = "Publishes a `AcceptorUdpPacketReceived` event to the publisher's subscriber"]
        fn on_acceptor_udp_packet_received(&self, event: builder::AcceptorUdpPacketReceived);
        #[doc = "Publishes a `AcceptorUdpPacketDropped` event to the publisher's subscriber"]
        fn on_acceptor_udp_packet_dropped(&self, event: builder::AcceptorUdpPacketDropped);
        #[doc = "Publishes a `AcceptorUdpStreamEnqueued` event to the publisher's subscriber"]
        fn on_acceptor_udp_stream_enqueued(&self, event: builder::AcceptorUdpStreamEnqueued);
        #[doc = "Publishes a `AcceptorUdpIoError` event to the publisher's subscriber"]
        fn on_acceptor_udp_io_error(&self, event: builder::AcceptorUdpIoError);
        #[doc = "Publishes a `AcceptorStreamPruned` event to the publisher's subscriber"]
        fn on_acceptor_stream_pruned(&self, event: builder::AcceptorStreamPruned);
        #[doc = "Publishes a `AcceptorStreamDequeued` event to the publisher's subscriber"]
        fn on_acceptor_stream_dequeued(&self, event: builder::AcceptorStreamDequeued);
        #[doc = "Publishes a `StreamTcpConnect` event to the publisher's subscriber"]
        fn on_stream_tcp_connect(&self, event: builder::StreamTcpConnect);
        #[doc = "Publishes a `StreamConnect` event to the publisher's subscriber"]
        fn on_stream_connect(&self, event: builder::StreamConnect);
        #[doc = "Publishes a `StreamConnectError` event to the publisher's subscriber"]
        fn on_stream_connect_error(&self, event: builder::StreamConnectError);
        #[doc = "Publishes a `EndpointInitialized` event to the publisher's subscriber"]
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized);
        #[doc = "Publishes a `PathSecretMapInitialized` event to the publisher's subscriber"]
        fn on_path_secret_map_initialized(&self, event: builder::PathSecretMapInitialized);
        #[doc = "Publishes a `PathSecretMapUninitialized` event to the publisher's subscriber"]
        fn on_path_secret_map_uninitialized(&self, event: builder::PathSecretMapUninitialized);
        #[doc = "Publishes a `PathSecretMapBackgroundHandshakeRequested` event to the publisher's subscriber"]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            event: builder::PathSecretMapBackgroundHandshakeRequested,
        );
        #[doc = "Publishes a `PathSecretMapEntryInserted` event to the publisher's subscriber"]
        fn on_path_secret_map_entry_inserted(&self, event: builder::PathSecretMapEntryInserted);
        #[doc = "Publishes a `PathSecretMapEntryReady` event to the publisher's subscriber"]
        fn on_path_secret_map_entry_ready(&self, event: builder::PathSecretMapEntryReady);
        #[doc = "Publishes a `PathSecretMapEntryReplaced` event to the publisher's subscriber"]
        fn on_path_secret_map_entry_replaced(&self, event: builder::PathSecretMapEntryReplaced);
        #[doc = "Publishes a `PathSecretMapIdEntryEvicted` event to the publisher's subscriber"]
        fn on_path_secret_map_id_entry_evicted(&self, event: builder::PathSecretMapIdEntryEvicted);
        #[doc = "Publishes a `PathSecretMapAddressEntryEvicted` event to the publisher's subscriber"]
        fn on_path_secret_map_address_entry_evicted(
            &self,
            event: builder::PathSecretMapAddressEntryEvicted,
        );
        #[doc = "Publishes a `UnknownPathSecretPacketSent` event to the publisher's subscriber"]
        fn on_unknown_path_secret_packet_sent(&self, event: builder::UnknownPathSecretPacketSent);
        #[doc = "Publishes a `UnknownPathSecretPacketReceived` event to the publisher's subscriber"]
        fn on_unknown_path_secret_packet_received(
            &self,
            event: builder::UnknownPathSecretPacketReceived,
        );
        #[doc = "Publishes a `UnknownPathSecretPacketAccepted` event to the publisher's subscriber"]
        fn on_unknown_path_secret_packet_accepted(
            &self,
            event: builder::UnknownPathSecretPacketAccepted,
        );
        #[doc = "Publishes a `UnknownPathSecretPacketRejected` event to the publisher's subscriber"]
        fn on_unknown_path_secret_packet_rejected(
            &self,
            event: builder::UnknownPathSecretPacketRejected,
        );
        #[doc = "Publishes a `UnknownPathSecretPacketDropped` event to the publisher's subscriber"]
        fn on_unknown_path_secret_packet_dropped(
            &self,
            event: builder::UnknownPathSecretPacketDropped,
        );
        #[doc = "Publishes a `KeyAccepted` event to the publisher's subscriber"]
        fn on_key_accepted(&self, event: builder::KeyAccepted);
        #[doc = "Publishes a `ReplayDefinitelyDetected` event to the publisher's subscriber"]
        fn on_replay_definitely_detected(&self, event: builder::ReplayDefinitelyDetected);
        #[doc = "Publishes a `ReplayPotentiallyDetected` event to the publisher's subscriber"]
        fn on_replay_potentially_detected(&self, event: builder::ReplayPotentiallyDetected);
        #[doc = "Publishes a `ReplayDetectedPacketSent` event to the publisher's subscriber"]
        fn on_replay_detected_packet_sent(&self, event: builder::ReplayDetectedPacketSent);
        #[doc = "Publishes a `ReplayDetectedPacketReceived` event to the publisher's subscriber"]
        fn on_replay_detected_packet_received(&self, event: builder::ReplayDetectedPacketReceived);
        #[doc = "Publishes a `ReplayDetectedPacketAccepted` event to the publisher's subscriber"]
        fn on_replay_detected_packet_accepted(&self, event: builder::ReplayDetectedPacketAccepted);
        #[doc = "Publishes a `ReplayDetectedPacketRejected` event to the publisher's subscriber"]
        fn on_replay_detected_packet_rejected(&self, event: builder::ReplayDetectedPacketRejected);
        #[doc = "Publishes a `ReplayDetectedPacketDropped` event to the publisher's subscriber"]
        fn on_replay_detected_packet_dropped(&self, event: builder::ReplayDetectedPacketDropped);
        #[doc = "Publishes a `StaleKeyPacketSent` event to the publisher's subscriber"]
        fn on_stale_key_packet_sent(&self, event: builder::StaleKeyPacketSent);
        #[doc = "Publishes a `StaleKeyPacketReceived` event to the publisher's subscriber"]
        fn on_stale_key_packet_received(&self, event: builder::StaleKeyPacketReceived);
        #[doc = "Publishes a `StaleKeyPacketAccepted` event to the publisher's subscriber"]
        fn on_stale_key_packet_accepted(&self, event: builder::StaleKeyPacketAccepted);
        #[doc = "Publishes a `StaleKeyPacketRejected` event to the publisher's subscriber"]
        fn on_stale_key_packet_rejected(&self, event: builder::StaleKeyPacketRejected);
        #[doc = "Publishes a `StaleKeyPacketDropped` event to the publisher's subscriber"]
        fn on_stale_key_packet_dropped(&self, event: builder::StaleKeyPacketDropped);
        #[doc = "Publishes a `PathSecretMapAddressCacheAccessed` event to the publisher's subscriber"]
        fn on_path_secret_map_address_cache_accessed(
            &self,
            event: builder::PathSecretMapAddressCacheAccessed,
        );
        #[doc = "Publishes a `PathSecretMapAddressCacheAccessedHit` event to the publisher's subscriber"]
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            event: builder::PathSecretMapAddressCacheAccessedHit,
        );
        #[doc = "Publishes a `PathSecretMapIdCacheAccessed` event to the publisher's subscriber"]
        fn on_path_secret_map_id_cache_accessed(
            &self,
            event: builder::PathSecretMapIdCacheAccessed,
        );
        #[doc = "Publishes a `PathSecretMapIdCacheAccessedHit` event to the publisher's subscriber"]
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            event: builder::PathSecretMapIdCacheAccessedHit,
        );
        #[doc = "Publishes a `PathSecretMapCleanerCycled` event to the publisher's subscriber"]
        fn on_path_secret_map_cleaner_cycled(&self, event: builder::PathSecretMapCleanerCycled);
        #[doc = r" Returns the QUIC version, if any"]
        fn quic_version(&self) -> Option<u32>;
    }
    pub struct EndpointPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::EndpointMeta,
        quic_version: Option<u32>,
        subscriber: &'a Sub,
    }
    impl<'a, Sub: Subscriber> fmt::Debug for EndpointPublisherSubscriber<'a, Sub> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("ConnectionPublisherSubscriber")
                .field("meta", &self.meta)
                .field("quic_version", &self.quic_version)
                .finish()
        }
    }
    impl<'a, Sub: Subscriber> EndpointPublisherSubscriber<'a, Sub> {
        #[inline]
        pub fn new(
            meta: builder::EndpointMeta,
            quic_version: Option<u32>,
            subscriber: &'a Sub,
        ) -> Self {
            Self {
                meta: meta.into_event(),
                quic_version,
                subscriber,
            }
        }
    }
    impl<'a, Sub: Subscriber> EndpointPublisher for EndpointPublisherSubscriber<'a, Sub> {
        #[inline]
        fn on_acceptor_tcp_started(&self, event: builder::AcceptorTcpStarted) {
            let event = event.into_event();
            self.subscriber.on_acceptor_tcp_started(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            event: builder::AcceptorTcpLoopIterationCompleted,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_loop_iteration_completed(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_fresh_enqueued(&self, event: builder::AcceptorTcpFreshEnqueued) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_fresh_enqueued(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            event: builder::AcceptorTcpFreshBatchCompleted,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_fresh_batch_completed(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_dropped(&self, event: builder::AcceptorTcpStreamDropped) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_stream_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_replaced(&self, event: builder::AcceptorTcpStreamReplaced) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_stream_replaced(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_packet_received(&self, event: builder::AcceptorTcpPacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_packet_dropped(&self, event: builder::AcceptorTcpPacketDropped) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_packet_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_stream_enqueued(&self, event: builder::AcceptorTcpStreamEnqueued) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_tcp_stream_enqueued(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_tcp_io_error(&self, event: builder::AcceptorTcpIoError) {
            let event = event.into_event();
            self.subscriber.on_acceptor_tcp_io_error(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_udp_started(&self, event: builder::AcceptorUdpStarted) {
            let event = event.into_event();
            self.subscriber.on_acceptor_udp_started(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_udp_datagram_received(&self, event: builder::AcceptorUdpDatagramReceived) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_udp_datagram_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_udp_packet_received(&self, event: builder::AcceptorUdpPacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_udp_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_udp_packet_dropped(&self, event: builder::AcceptorUdpPacketDropped) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_udp_packet_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_udp_stream_enqueued(&self, event: builder::AcceptorUdpStreamEnqueued) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_udp_stream_enqueued(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_udp_io_error(&self, event: builder::AcceptorUdpIoError) {
            let event = event.into_event();
            self.subscriber.on_acceptor_udp_io_error(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_stream_pruned(&self, event: builder::AcceptorStreamPruned) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_stream_pruned(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_acceptor_stream_dequeued(&self, event: builder::AcceptorStreamDequeued) {
            let event = event.into_event();
            self.subscriber
                .on_acceptor_stream_dequeued(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_tcp_connect(&self, event: builder::StreamTcpConnect) {
            let event = event.into_event();
            self.subscriber.on_stream_tcp_connect(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_connect(&self, event: builder::StreamConnect) {
            let event = event.into_event();
            self.subscriber.on_stream_connect(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_connect_error(&self, event: builder::StreamConnectError) {
            let event = event.into_event();
            self.subscriber.on_stream_connect_error(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            let event = event.into_event();
            self.subscriber.on_endpoint_initialized(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_initialized(&self, event: builder::PathSecretMapInitialized) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_initialized(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_uninitialized(&self, event: builder::PathSecretMapUninitialized) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_uninitialized(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            event: builder::PathSecretMapBackgroundHandshakeRequested,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_background_handshake_requested(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_entry_inserted(&self, event: builder::PathSecretMapEntryInserted) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_entry_inserted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_entry_ready(&self, event: builder::PathSecretMapEntryReady) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_entry_ready(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_entry_replaced(&self, event: builder::PathSecretMapEntryReplaced) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_entry_replaced(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_id_entry_evicted(&self, event: builder::PathSecretMapIdEntryEvicted) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_id_entry_evicted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_address_entry_evicted(
            &self,
            event: builder::PathSecretMapAddressEntryEvicted,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_address_entry_evicted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_sent(&self, event: builder::UnknownPathSecretPacketSent) {
            let event = event.into_event();
            self.subscriber
                .on_unknown_path_secret_packet_sent(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_received(
            &self,
            event: builder::UnknownPathSecretPacketReceived,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_unknown_path_secret_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_accepted(
            &self,
            event: builder::UnknownPathSecretPacketAccepted,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_unknown_path_secret_packet_accepted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_rejected(
            &self,
            event: builder::UnknownPathSecretPacketRejected,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_unknown_path_secret_packet_rejected(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_unknown_path_secret_packet_dropped(
            &self,
            event: builder::UnknownPathSecretPacketDropped,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_unknown_path_secret_packet_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_key_accepted(&self, event: builder::KeyAccepted) {
            let event = event.into_event();
            self.subscriber.on_key_accepted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_definitely_detected(&self, event: builder::ReplayDefinitelyDetected) {
            let event = event.into_event();
            self.subscriber
                .on_replay_definitely_detected(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_potentially_detected(&self, event: builder::ReplayPotentiallyDetected) {
            let event = event.into_event();
            self.subscriber
                .on_replay_potentially_detected(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_detected_packet_sent(&self, event: builder::ReplayDetectedPacketSent) {
            let event = event.into_event();
            self.subscriber
                .on_replay_detected_packet_sent(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_detected_packet_received(&self, event: builder::ReplayDetectedPacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_replay_detected_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_detected_packet_accepted(&self, event: builder::ReplayDetectedPacketAccepted) {
            let event = event.into_event();
            self.subscriber
                .on_replay_detected_packet_accepted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_detected_packet_rejected(&self, event: builder::ReplayDetectedPacketRejected) {
            let event = event.into_event();
            self.subscriber
                .on_replay_detected_packet_rejected(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_replay_detected_packet_dropped(&self, event: builder::ReplayDetectedPacketDropped) {
            let event = event.into_event();
            self.subscriber
                .on_replay_detected_packet_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stale_key_packet_sent(&self, event: builder::StaleKeyPacketSent) {
            let event = event.into_event();
            self.subscriber.on_stale_key_packet_sent(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stale_key_packet_received(&self, event: builder::StaleKeyPacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_stale_key_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stale_key_packet_accepted(&self, event: builder::StaleKeyPacketAccepted) {
            let event = event.into_event();
            self.subscriber
                .on_stale_key_packet_accepted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stale_key_packet_rejected(&self, event: builder::StaleKeyPacketRejected) {
            let event = event.into_event();
            self.subscriber
                .on_stale_key_packet_rejected(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stale_key_packet_dropped(&self, event: builder::StaleKeyPacketDropped) {
            let event = event.into_event();
            self.subscriber
                .on_stale_key_packet_dropped(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed(
            &self,
            event: builder::PathSecretMapAddressCacheAccessed,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_address_cache_accessed(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            event: builder::PathSecretMapAddressCacheAccessedHit,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_address_cache_accessed_hit(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed(
            &self,
            event: builder::PathSecretMapIdCacheAccessed,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_id_cache_accessed(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            event: builder::PathSecretMapIdCacheAccessedHit,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_id_cache_accessed_hit(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_cleaner_cycled(&self, event: builder::PathSecretMapCleanerCycled) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_cleaner_cycled(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `StreamWriteFlushed` event to the publisher's subscriber"]
        fn on_stream_write_flushed(&self, event: builder::StreamWriteFlushed);
        #[doc = "Publishes a `StreamWriteFinFlushed` event to the publisher's subscriber"]
        fn on_stream_write_fin_flushed(&self, event: builder::StreamWriteFinFlushed);
        #[doc = "Publishes a `StreamWriteBlocked` event to the publisher's subscriber"]
        fn on_stream_write_blocked(&self, event: builder::StreamWriteBlocked);
        #[doc = "Publishes a `StreamWriteErrored` event to the publisher's subscriber"]
        fn on_stream_write_errored(&self, event: builder::StreamWriteErrored);
        #[doc = "Publishes a `StreamWriteKeyUpdated` event to the publisher's subscriber"]
        fn on_stream_write_key_updated(&self, event: builder::StreamWriteKeyUpdated);
        #[doc = "Publishes a `StreamWriteShutdown` event to the publisher's subscriber"]
        fn on_stream_write_shutdown(&self, event: builder::StreamWriteShutdown);
        #[doc = "Publishes a `StreamWriteSocketFlushed` event to the publisher's subscriber"]
        fn on_stream_write_socket_flushed(&self, event: builder::StreamWriteSocketFlushed);
        #[doc = "Publishes a `StreamWriteSocketBlocked` event to the publisher's subscriber"]
        fn on_stream_write_socket_blocked(&self, event: builder::StreamWriteSocketBlocked);
        #[doc = "Publishes a `StreamWriteSocketErrored` event to the publisher's subscriber"]
        fn on_stream_write_socket_errored(&self, event: builder::StreamWriteSocketErrored);
        #[doc = "Publishes a `StreamReadFlushed` event to the publisher's subscriber"]
        fn on_stream_read_flushed(&self, event: builder::StreamReadFlushed);
        #[doc = "Publishes a `StreamReadFinFlushed` event to the publisher's subscriber"]
        fn on_stream_read_fin_flushed(&self, event: builder::StreamReadFinFlushed);
        #[doc = "Publishes a `StreamReadBlocked` event to the publisher's subscriber"]
        fn on_stream_read_blocked(&self, event: builder::StreamReadBlocked);
        #[doc = "Publishes a `StreamReadErrored` event to the publisher's subscriber"]
        fn on_stream_read_errored(&self, event: builder::StreamReadErrored);
        #[doc = "Publishes a `StreamReadKeyUpdated` event to the publisher's subscriber"]
        fn on_stream_read_key_updated(&self, event: builder::StreamReadKeyUpdated);
        #[doc = "Publishes a `StreamReadShutdown` event to the publisher's subscriber"]
        fn on_stream_read_shutdown(&self, event: builder::StreamReadShutdown);
        #[doc = "Publishes a `StreamReadSocketFlushed` event to the publisher's subscriber"]
        fn on_stream_read_socket_flushed(&self, event: builder::StreamReadSocketFlushed);
        #[doc = "Publishes a `StreamReadSocketBlocked` event to the publisher's subscriber"]
        fn on_stream_read_socket_blocked(&self, event: builder::StreamReadSocketBlocked);
        #[doc = "Publishes a `StreamReadSocketErrored` event to the publisher's subscriber"]
        fn on_stream_read_socket_errored(&self, event: builder::StreamReadSocketErrored);
        #[doc = "Publishes a `ConnectionClosed` event to the publisher's subscriber"]
        fn on_connection_closed(&self, event: builder::ConnectionClosed);
        #[doc = r" Returns the QUIC version negotiated for the current connection, if any"]
        fn quic_version(&self) -> u32;
        #[doc = r" Returns the [`Subject`] for the current publisher"]
        fn subject(&self) -> api::Subject;
    }
    pub struct ConnectionPublisherSubscriber<'a, Sub: Subscriber> {
        meta: api::ConnectionMeta,
        quic_version: u32,
        subscriber: &'a Sub,
        context: &'a Sub::ConnectionContext,
    }
    impl<'a, Sub: Subscriber> fmt::Debug for ConnectionPublisherSubscriber<'a, Sub> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("ConnectionPublisherSubscriber")
                .field("meta", &self.meta)
                .field("quic_version", &self.quic_version)
                .finish()
        }
    }
    impl<'a, Sub: Subscriber> ConnectionPublisherSubscriber<'a, Sub> {
        #[inline]
        pub fn new(
            meta: builder::ConnectionMeta,
            quic_version: u32,
            subscriber: &'a Sub,
            context: &'a Sub::ConnectionContext,
        ) -> Self {
            Self {
                meta: meta.into_event(),
                quic_version,
                subscriber,
                context,
            }
        }
    }
    impl<'a, Sub: Subscriber> ConnectionPublisher for ConnectionPublisherSubscriber<'a, Sub> {
        #[inline]
        fn on_stream_write_flushed(&self, event: builder::StreamWriteFlushed) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_flushed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_fin_flushed(&self, event: builder::StreamWriteFinFlushed) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_fin_flushed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_blocked(&self, event: builder::StreamWriteBlocked) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_blocked(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_errored(&self, event: builder::StreamWriteErrored) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_errored(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_key_updated(&self, event: builder::StreamWriteKeyUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_key_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_shutdown(&self, event: builder::StreamWriteShutdown) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_shutdown(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_socket_flushed(&self, event: builder::StreamWriteSocketFlushed) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_socket_flushed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_socket_blocked(&self, event: builder::StreamWriteSocketBlocked) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_socket_blocked(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_write_socket_errored(&self, event: builder::StreamWriteSocketErrored) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_socket_errored(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_flushed(&self, event: builder::StreamReadFlushed) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_flushed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_fin_flushed(&self, event: builder::StreamReadFinFlushed) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_fin_flushed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_blocked(&self, event: builder::StreamReadBlocked) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_blocked(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_errored(&self, event: builder::StreamReadErrored) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_errored(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_key_updated(&self, event: builder::StreamReadKeyUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_key_updated(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_shutdown(&self, event: builder::StreamReadShutdown) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_shutdown(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_socket_flushed(&self, event: builder::StreamReadSocketFlushed) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_socket_flushed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_socket_blocked(&self, event: builder::StreamReadSocketBlocked) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_socket_blocked(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_stream_read_socket_errored(&self, event: builder::StreamReadSocketErrored) {
            let event = event.into_event();
            self.subscriber
                .on_stream_read_socket_errored(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_connection_closed(&self, event: builder::ConnectionClosed) {
            let event = event.into_event();
            self.subscriber
                .on_connection_closed(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> u32 {
            self.quic_version
        }
        #[inline]
        fn subject(&self) -> api::Subject {
            self.meta.subject()
        }
    }
}
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::event::snapshot::Location;
    use core::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    pub mod endpoint {
        use super::*;
        pub struct Subscriber {
            location: Option<Location>,
            output: Mutex<Vec<String>>,
            pub acceptor_tcp_started: AtomicU64,
            pub acceptor_tcp_loop_iteration_completed: AtomicU64,
            pub acceptor_tcp_fresh_enqueued: AtomicU64,
            pub acceptor_tcp_fresh_batch_completed: AtomicU64,
            pub acceptor_tcp_stream_dropped: AtomicU64,
            pub acceptor_tcp_stream_replaced: AtomicU64,
            pub acceptor_tcp_packet_received: AtomicU64,
            pub acceptor_tcp_packet_dropped: AtomicU64,
            pub acceptor_tcp_stream_enqueued: AtomicU64,
            pub acceptor_tcp_io_error: AtomicU64,
            pub acceptor_udp_started: AtomicU64,
            pub acceptor_udp_datagram_received: AtomicU64,
            pub acceptor_udp_packet_received: AtomicU64,
            pub acceptor_udp_packet_dropped: AtomicU64,
            pub acceptor_udp_stream_enqueued: AtomicU64,
            pub acceptor_udp_io_error: AtomicU64,
            pub acceptor_stream_pruned: AtomicU64,
            pub acceptor_stream_dequeued: AtomicU64,
            pub stream_tcp_connect: AtomicU64,
            pub stream_connect: AtomicU64,
            pub stream_connect_error: AtomicU64,
            pub endpoint_initialized: AtomicU64,
            pub path_secret_map_initialized: AtomicU64,
            pub path_secret_map_uninitialized: AtomicU64,
            pub path_secret_map_background_handshake_requested: AtomicU64,
            pub path_secret_map_entry_inserted: AtomicU64,
            pub path_secret_map_entry_ready: AtomicU64,
            pub path_secret_map_entry_replaced: AtomicU64,
            pub path_secret_map_id_entry_evicted: AtomicU64,
            pub path_secret_map_address_entry_evicted: AtomicU64,
            pub unknown_path_secret_packet_sent: AtomicU64,
            pub unknown_path_secret_packet_received: AtomicU64,
            pub unknown_path_secret_packet_accepted: AtomicU64,
            pub unknown_path_secret_packet_rejected: AtomicU64,
            pub unknown_path_secret_packet_dropped: AtomicU64,
            pub key_accepted: AtomicU64,
            pub replay_definitely_detected: AtomicU64,
            pub replay_potentially_detected: AtomicU64,
            pub replay_detected_packet_sent: AtomicU64,
            pub replay_detected_packet_received: AtomicU64,
            pub replay_detected_packet_accepted: AtomicU64,
            pub replay_detected_packet_rejected: AtomicU64,
            pub replay_detected_packet_dropped: AtomicU64,
            pub stale_key_packet_sent: AtomicU64,
            pub stale_key_packet_received: AtomicU64,
            pub stale_key_packet_accepted: AtomicU64,
            pub stale_key_packet_rejected: AtomicU64,
            pub stale_key_packet_dropped: AtomicU64,
            pub path_secret_map_address_cache_accessed: AtomicU64,
            pub path_secret_map_address_cache_accessed_hit: AtomicU64,
            pub path_secret_map_id_cache_accessed: AtomicU64,
            pub path_secret_map_id_cache_accessed_hit: AtomicU64,
            pub path_secret_map_cleaner_cycled: AtomicU64,
        }
        impl Drop for Subscriber {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    return;
                }
                if let Some(location) = self.location.as_ref() {
                    location.snapshot_log(&self.output.lock().unwrap());
                }
            }
        }
        impl Subscriber {
            #[doc = r" Creates a subscriber with snapshot assertions enabled"]
            #[track_caller]
            pub fn snapshot() -> Self {
                let mut sub = Self::no_snapshot();
                sub.location = Location::from_thread_name();
                sub
            }
            #[doc = r" Creates a subscriber with snapshot assertions enabled"]
            #[track_caller]
            pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
                let mut sub = Self::no_snapshot();
                sub.location = Some(Location::new(name));
                sub
            }
            #[doc = r" Creates a subscriber with snapshot assertions disabled"]
            pub fn no_snapshot() -> Self {
                Self {
                    location: None,
                    output: Default::default(),
                    acceptor_tcp_started: AtomicU64::new(0),
                    acceptor_tcp_loop_iteration_completed: AtomicU64::new(0),
                    acceptor_tcp_fresh_enqueued: AtomicU64::new(0),
                    acceptor_tcp_fresh_batch_completed: AtomicU64::new(0),
                    acceptor_tcp_stream_dropped: AtomicU64::new(0),
                    acceptor_tcp_stream_replaced: AtomicU64::new(0),
                    acceptor_tcp_packet_received: AtomicU64::new(0),
                    acceptor_tcp_packet_dropped: AtomicU64::new(0),
                    acceptor_tcp_stream_enqueued: AtomicU64::new(0),
                    acceptor_tcp_io_error: AtomicU64::new(0),
                    acceptor_udp_started: AtomicU64::new(0),
                    acceptor_udp_datagram_received: AtomicU64::new(0),
                    acceptor_udp_packet_received: AtomicU64::new(0),
                    acceptor_udp_packet_dropped: AtomicU64::new(0),
                    acceptor_udp_stream_enqueued: AtomicU64::new(0),
                    acceptor_udp_io_error: AtomicU64::new(0),
                    acceptor_stream_pruned: AtomicU64::new(0),
                    acceptor_stream_dequeued: AtomicU64::new(0),
                    stream_tcp_connect: AtomicU64::new(0),
                    stream_connect: AtomicU64::new(0),
                    stream_connect_error: AtomicU64::new(0),
                    endpoint_initialized: AtomicU64::new(0),
                    path_secret_map_initialized: AtomicU64::new(0),
                    path_secret_map_uninitialized: AtomicU64::new(0),
                    path_secret_map_background_handshake_requested: AtomicU64::new(0),
                    path_secret_map_entry_inserted: AtomicU64::new(0),
                    path_secret_map_entry_ready: AtomicU64::new(0),
                    path_secret_map_entry_replaced: AtomicU64::new(0),
                    path_secret_map_id_entry_evicted: AtomicU64::new(0),
                    path_secret_map_address_entry_evicted: AtomicU64::new(0),
                    unknown_path_secret_packet_sent: AtomicU64::new(0),
                    unknown_path_secret_packet_received: AtomicU64::new(0),
                    unknown_path_secret_packet_accepted: AtomicU64::new(0),
                    unknown_path_secret_packet_rejected: AtomicU64::new(0),
                    unknown_path_secret_packet_dropped: AtomicU64::new(0),
                    key_accepted: AtomicU64::new(0),
                    replay_definitely_detected: AtomicU64::new(0),
                    replay_potentially_detected: AtomicU64::new(0),
                    replay_detected_packet_sent: AtomicU64::new(0),
                    replay_detected_packet_received: AtomicU64::new(0),
                    replay_detected_packet_accepted: AtomicU64::new(0),
                    replay_detected_packet_rejected: AtomicU64::new(0),
                    replay_detected_packet_dropped: AtomicU64::new(0),
                    stale_key_packet_sent: AtomicU64::new(0),
                    stale_key_packet_received: AtomicU64::new(0),
                    stale_key_packet_accepted: AtomicU64::new(0),
                    stale_key_packet_rejected: AtomicU64::new(0),
                    stale_key_packet_dropped: AtomicU64::new(0),
                    path_secret_map_address_cache_accessed: AtomicU64::new(0),
                    path_secret_map_address_cache_accessed_hit: AtomicU64::new(0),
                    path_secret_map_id_cache_accessed: AtomicU64::new(0),
                    path_secret_map_id_cache_accessed_hit: AtomicU64::new(0),
                    path_secret_map_cleaner_cycled: AtomicU64::new(0),
                }
            }
        }
        impl super::super::Subscriber for Subscriber {
            type ConnectionContext = ();
            fn create_connection_context(
                &self,
                _meta: &api::ConnectionMeta,
                _info: &api::ConnectionInfo,
            ) -> Self::ConnectionContext {
            }
            fn on_acceptor_tcp_started(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpStarted,
            ) {
                self.acceptor_tcp_started.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_loop_iteration_completed(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpLoopIterationCompleted,
            ) {
                self.acceptor_tcp_loop_iteration_completed
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_fresh_enqueued(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpFreshEnqueued,
            ) {
                self.acceptor_tcp_fresh_enqueued
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_fresh_batch_completed(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpFreshBatchCompleted,
            ) {
                self.acceptor_tcp_fresh_batch_completed
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_stream_dropped(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpStreamDropped,
            ) {
                self.acceptor_tcp_stream_dropped
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_stream_replaced(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpStreamReplaced,
            ) {
                self.acceptor_tcp_stream_replaced
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_packet_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpPacketReceived,
            ) {
                self.acceptor_tcp_packet_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_packet_dropped(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpPacketDropped,
            ) {
                self.acceptor_tcp_packet_dropped
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_stream_enqueued(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpStreamEnqueued,
            ) {
                self.acceptor_tcp_stream_enqueued
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_tcp_io_error(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorTcpIoError,
            ) {
                self.acceptor_tcp_io_error.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_udp_started(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorUdpStarted,
            ) {
                self.acceptor_udp_started.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_udp_datagram_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorUdpDatagramReceived,
            ) {
                self.acceptor_udp_datagram_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_udp_packet_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorUdpPacketReceived,
            ) {
                self.acceptor_udp_packet_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_udp_packet_dropped(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorUdpPacketDropped,
            ) {
                self.acceptor_udp_packet_dropped
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_udp_stream_enqueued(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorUdpStreamEnqueued,
            ) {
                self.acceptor_udp_stream_enqueued
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_udp_io_error(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorUdpIoError,
            ) {
                self.acceptor_udp_io_error.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_stream_pruned(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorStreamPruned,
            ) {
                self.acceptor_stream_pruned.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_acceptor_stream_dequeued(
                &self,
                meta: &api::EndpointMeta,
                event: &api::AcceptorStreamDequeued,
            ) {
                self.acceptor_stream_dequeued
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stream_tcp_connect(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StreamTcpConnect,
            ) {
                self.stream_tcp_connect.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
                self.stream_connect.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stream_connect_error(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StreamConnectError,
            ) {
                self.stream_connect_error.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_initialized(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointInitialized,
            ) {
                self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_initialized(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapInitialized,
            ) {
                self.path_secret_map_initialized
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_uninitialized(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapUninitialized,
            ) {
                self.path_secret_map_uninitialized
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_background_handshake_requested(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapBackgroundHandshakeRequested,
            ) {
                self.path_secret_map_background_handshake_requested
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_entry_inserted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapEntryInserted,
            ) {
                self.path_secret_map_entry_inserted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_entry_ready(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapEntryReady,
            ) {
                self.path_secret_map_entry_ready
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_entry_replaced(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapEntryReplaced,
            ) {
                self.path_secret_map_entry_replaced
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_id_entry_evicted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapIdEntryEvicted,
            ) {
                self.path_secret_map_id_entry_evicted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_address_entry_evicted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapAddressEntryEvicted,
            ) {
                self.path_secret_map_address_entry_evicted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_unknown_path_secret_packet_sent(
                &self,
                meta: &api::EndpointMeta,
                event: &api::UnknownPathSecretPacketSent,
            ) {
                self.unknown_path_secret_packet_sent
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_unknown_path_secret_packet_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::UnknownPathSecretPacketReceived,
            ) {
                self.unknown_path_secret_packet_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_unknown_path_secret_packet_accepted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::UnknownPathSecretPacketAccepted,
            ) {
                self.unknown_path_secret_packet_accepted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_unknown_path_secret_packet_rejected(
                &self,
                meta: &api::EndpointMeta,
                event: &api::UnknownPathSecretPacketRejected,
            ) {
                self.unknown_path_secret_packet_rejected
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_unknown_path_secret_packet_dropped(
                &self,
                meta: &api::EndpointMeta,
                event: &api::UnknownPathSecretPacketDropped,
            ) {
                self.unknown_path_secret_packet_dropped
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
                self.key_accepted.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_definitely_detected(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayDefinitelyDetected,
            ) {
                self.replay_definitely_detected
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_potentially_detected(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayPotentiallyDetected,
            ) {
                self.replay_potentially_detected
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_detected_packet_sent(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayDetectedPacketSent,
            ) {
                self.replay_detected_packet_sent
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_detected_packet_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayDetectedPacketReceived,
            ) {
                self.replay_detected_packet_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_detected_packet_accepted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayDetectedPacketAccepted,
            ) {
                self.replay_detected_packet_accepted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_detected_packet_rejected(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayDetectedPacketRejected,
            ) {
                self.replay_detected_packet_rejected
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_replay_detected_packet_dropped(
                &self,
                meta: &api::EndpointMeta,
                event: &api::ReplayDetectedPacketDropped,
            ) {
                self.replay_detected_packet_dropped
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stale_key_packet_sent(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StaleKeyPacketSent,
            ) {
                self.stale_key_packet_sent.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stale_key_packet_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StaleKeyPacketReceived,
            ) {
                self.stale_key_packet_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stale_key_packet_accepted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StaleKeyPacketAccepted,
            ) {
                self.stale_key_packet_accepted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stale_key_packet_rejected(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StaleKeyPacketRejected,
            ) {
                self.stale_key_packet_rejected
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_stale_key_packet_dropped(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StaleKeyPacketDropped,
            ) {
                self.stale_key_packet_dropped
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_address_cache_accessed(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapAddressCacheAccessed,
            ) {
                self.path_secret_map_address_cache_accessed
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_address_cache_accessed_hit(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapAddressCacheAccessedHit,
            ) {
                self.path_secret_map_address_cache_accessed_hit
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_id_cache_accessed(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapIdCacheAccessed,
            ) {
                self.path_secret_map_id_cache_accessed
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_id_cache_accessed_hit(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapIdCacheAccessedHit,
            ) {
                self.path_secret_map_id_cache_accessed_hit
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_cleaner_cycled(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapCleanerCycled,
            ) {
                self.path_secret_map_cleaner_cycled
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
    }
    #[derive(Debug)]
    pub struct Subscriber {
        location: Option<Location>,
        output: Mutex<Vec<String>>,
        pub acceptor_tcp_started: AtomicU64,
        pub acceptor_tcp_loop_iteration_completed: AtomicU64,
        pub acceptor_tcp_fresh_enqueued: AtomicU64,
        pub acceptor_tcp_fresh_batch_completed: AtomicU64,
        pub acceptor_tcp_stream_dropped: AtomicU64,
        pub acceptor_tcp_stream_replaced: AtomicU64,
        pub acceptor_tcp_packet_received: AtomicU64,
        pub acceptor_tcp_packet_dropped: AtomicU64,
        pub acceptor_tcp_stream_enqueued: AtomicU64,
        pub acceptor_tcp_io_error: AtomicU64,
        pub acceptor_udp_started: AtomicU64,
        pub acceptor_udp_datagram_received: AtomicU64,
        pub acceptor_udp_packet_received: AtomicU64,
        pub acceptor_udp_packet_dropped: AtomicU64,
        pub acceptor_udp_stream_enqueued: AtomicU64,
        pub acceptor_udp_io_error: AtomicU64,
        pub acceptor_stream_pruned: AtomicU64,
        pub acceptor_stream_dequeued: AtomicU64,
        pub stream_write_flushed: AtomicU64,
        pub stream_write_fin_flushed: AtomicU64,
        pub stream_write_blocked: AtomicU64,
        pub stream_write_errored: AtomicU64,
        pub stream_write_key_updated: AtomicU64,
        pub stream_write_shutdown: AtomicU64,
        pub stream_write_socket_flushed: AtomicU64,
        pub stream_write_socket_blocked: AtomicU64,
        pub stream_write_socket_errored: AtomicU64,
        pub stream_read_flushed: AtomicU64,
        pub stream_read_fin_flushed: AtomicU64,
        pub stream_read_blocked: AtomicU64,
        pub stream_read_errored: AtomicU64,
        pub stream_read_key_updated: AtomicU64,
        pub stream_read_shutdown: AtomicU64,
        pub stream_read_socket_flushed: AtomicU64,
        pub stream_read_socket_blocked: AtomicU64,
        pub stream_read_socket_errored: AtomicU64,
        pub stream_tcp_connect: AtomicU64,
        pub stream_connect: AtomicU64,
        pub stream_connect_error: AtomicU64,
        pub connection_closed: AtomicU64,
        pub endpoint_initialized: AtomicU64,
        pub path_secret_map_initialized: AtomicU64,
        pub path_secret_map_uninitialized: AtomicU64,
        pub path_secret_map_background_handshake_requested: AtomicU64,
        pub path_secret_map_entry_inserted: AtomicU64,
        pub path_secret_map_entry_ready: AtomicU64,
        pub path_secret_map_entry_replaced: AtomicU64,
        pub path_secret_map_id_entry_evicted: AtomicU64,
        pub path_secret_map_address_entry_evicted: AtomicU64,
        pub unknown_path_secret_packet_sent: AtomicU64,
        pub unknown_path_secret_packet_received: AtomicU64,
        pub unknown_path_secret_packet_accepted: AtomicU64,
        pub unknown_path_secret_packet_rejected: AtomicU64,
        pub unknown_path_secret_packet_dropped: AtomicU64,
        pub key_accepted: AtomicU64,
        pub replay_definitely_detected: AtomicU64,
        pub replay_potentially_detected: AtomicU64,
        pub replay_detected_packet_sent: AtomicU64,
        pub replay_detected_packet_received: AtomicU64,
        pub replay_detected_packet_accepted: AtomicU64,
        pub replay_detected_packet_rejected: AtomicU64,
        pub replay_detected_packet_dropped: AtomicU64,
        pub stale_key_packet_sent: AtomicU64,
        pub stale_key_packet_received: AtomicU64,
        pub stale_key_packet_accepted: AtomicU64,
        pub stale_key_packet_rejected: AtomicU64,
        pub stale_key_packet_dropped: AtomicU64,
        pub path_secret_map_address_cache_accessed: AtomicU64,
        pub path_secret_map_address_cache_accessed_hit: AtomicU64,
        pub path_secret_map_id_cache_accessed: AtomicU64,
        pub path_secret_map_id_cache_accessed_hit: AtomicU64,
        pub path_secret_map_cleaner_cycled: AtomicU64,
    }
    impl Drop for Subscriber {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output.lock().unwrap());
            }
        }
    }
    impl Subscriber {
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::from_thread_name();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Some(Location::new(name));
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                acceptor_tcp_started: AtomicU64::new(0),
                acceptor_tcp_loop_iteration_completed: AtomicU64::new(0),
                acceptor_tcp_fresh_enqueued: AtomicU64::new(0),
                acceptor_tcp_fresh_batch_completed: AtomicU64::new(0),
                acceptor_tcp_stream_dropped: AtomicU64::new(0),
                acceptor_tcp_stream_replaced: AtomicU64::new(0),
                acceptor_tcp_packet_received: AtomicU64::new(0),
                acceptor_tcp_packet_dropped: AtomicU64::new(0),
                acceptor_tcp_stream_enqueued: AtomicU64::new(0),
                acceptor_tcp_io_error: AtomicU64::new(0),
                acceptor_udp_started: AtomicU64::new(0),
                acceptor_udp_datagram_received: AtomicU64::new(0),
                acceptor_udp_packet_received: AtomicU64::new(0),
                acceptor_udp_packet_dropped: AtomicU64::new(0),
                acceptor_udp_stream_enqueued: AtomicU64::new(0),
                acceptor_udp_io_error: AtomicU64::new(0),
                acceptor_stream_pruned: AtomicU64::new(0),
                acceptor_stream_dequeued: AtomicU64::new(0),
                stream_write_flushed: AtomicU64::new(0),
                stream_write_fin_flushed: AtomicU64::new(0),
                stream_write_blocked: AtomicU64::new(0),
                stream_write_errored: AtomicU64::new(0),
                stream_write_key_updated: AtomicU64::new(0),
                stream_write_shutdown: AtomicU64::new(0),
                stream_write_socket_flushed: AtomicU64::new(0),
                stream_write_socket_blocked: AtomicU64::new(0),
                stream_write_socket_errored: AtomicU64::new(0),
                stream_read_flushed: AtomicU64::new(0),
                stream_read_fin_flushed: AtomicU64::new(0),
                stream_read_blocked: AtomicU64::new(0),
                stream_read_errored: AtomicU64::new(0),
                stream_read_key_updated: AtomicU64::new(0),
                stream_read_shutdown: AtomicU64::new(0),
                stream_read_socket_flushed: AtomicU64::new(0),
                stream_read_socket_blocked: AtomicU64::new(0),
                stream_read_socket_errored: AtomicU64::new(0),
                stream_tcp_connect: AtomicU64::new(0),
                stream_connect: AtomicU64::new(0),
                stream_connect_error: AtomicU64::new(0),
                connection_closed: AtomicU64::new(0),
                endpoint_initialized: AtomicU64::new(0),
                path_secret_map_initialized: AtomicU64::new(0),
                path_secret_map_uninitialized: AtomicU64::new(0),
                path_secret_map_background_handshake_requested: AtomicU64::new(0),
                path_secret_map_entry_inserted: AtomicU64::new(0),
                path_secret_map_entry_ready: AtomicU64::new(0),
                path_secret_map_entry_replaced: AtomicU64::new(0),
                path_secret_map_id_entry_evicted: AtomicU64::new(0),
                path_secret_map_address_entry_evicted: AtomicU64::new(0),
                unknown_path_secret_packet_sent: AtomicU64::new(0),
                unknown_path_secret_packet_received: AtomicU64::new(0),
                unknown_path_secret_packet_accepted: AtomicU64::new(0),
                unknown_path_secret_packet_rejected: AtomicU64::new(0),
                unknown_path_secret_packet_dropped: AtomicU64::new(0),
                key_accepted: AtomicU64::new(0),
                replay_definitely_detected: AtomicU64::new(0),
                replay_potentially_detected: AtomicU64::new(0),
                replay_detected_packet_sent: AtomicU64::new(0),
                replay_detected_packet_received: AtomicU64::new(0),
                replay_detected_packet_accepted: AtomicU64::new(0),
                replay_detected_packet_rejected: AtomicU64::new(0),
                replay_detected_packet_dropped: AtomicU64::new(0),
                stale_key_packet_sent: AtomicU64::new(0),
                stale_key_packet_received: AtomicU64::new(0),
                stale_key_packet_accepted: AtomicU64::new(0),
                stale_key_packet_rejected: AtomicU64::new(0),
                stale_key_packet_dropped: AtomicU64::new(0),
                path_secret_map_address_cache_accessed: AtomicU64::new(0),
                path_secret_map_address_cache_accessed_hit: AtomicU64::new(0),
                path_secret_map_id_cache_accessed: AtomicU64::new(0),
                path_secret_map_id_cache_accessed_hit: AtomicU64::new(0),
                path_secret_map_cleaner_cycled: AtomicU64::new(0),
            }
        }
    }
    impl super::Subscriber for Subscriber {
        type ConnectionContext = ();
        fn create_connection_context(
            &self,
            _meta: &api::ConnectionMeta,
            _info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }
        fn on_acceptor_tcp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStarted,
        ) {
            self.acceptor_tcp_started.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpLoopIterationCompleted,
        ) {
            self.acceptor_tcp_loop_iteration_completed
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_fresh_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshEnqueued,
        ) {
            self.acceptor_tcp_fresh_enqueued
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpFreshBatchCompleted,
        ) {
            self.acceptor_tcp_fresh_batch_completed
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_stream_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamDropped,
        ) {
            self.acceptor_tcp_stream_dropped
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_stream_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamReplaced,
        ) {
            self.acceptor_tcp_stream_replaced
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketReceived,
        ) {
            self.acceptor_tcp_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpPacketDropped,
        ) {
            self.acceptor_tcp_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpStreamEnqueued,
        ) {
            self.acceptor_tcp_stream_enqueued
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorTcpIoError,
        ) {
            self.acceptor_tcp_io_error.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_started(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStarted,
        ) {
            self.acceptor_udp_started.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_datagram_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpDatagramReceived,
        ) {
            self.acceptor_udp_datagram_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketReceived,
        ) {
            self.acceptor_udp_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpPacketDropped,
        ) {
            self.acceptor_udp_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_stream_enqueued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpStreamEnqueued,
        ) {
            self.acceptor_udp_stream_enqueued
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_io_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorUdpIoError,
        ) {
            self.acceptor_udp_io_error.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_stream_pruned(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamPruned,
        ) {
            self.acceptor_stream_pruned.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_stream_dequeued(
            &self,
            meta: &api::EndpointMeta,
            event: &api::AcceptorStreamDequeued,
        ) {
            self.acceptor_stream_dequeued
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stream_write_flushed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFlushed,
        ) {
            self.stream_write_flushed.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_fin_flushed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteFinFlushed,
        ) {
            self.stream_write_fin_flushed
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_blocked(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteBlocked,
        ) {
            self.stream_write_blocked.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_errored(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteErrored,
        ) {
            self.stream_write_errored.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_key_updated(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteKeyUpdated,
        ) {
            self.stream_write_key_updated
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_shutdown(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteShutdown,
        ) {
            self.stream_write_shutdown.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_socket_flushed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketFlushed,
        ) {
            self.stream_write_socket_flushed
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_socket_blocked(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketBlocked,
        ) {
            self.stream_write_socket_blocked
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_socket_errored(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamWriteSocketErrored,
        ) {
            self.stream_write_socket_errored
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_flushed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFlushed,
        ) {
            self.stream_read_flushed.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_fin_flushed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadFinFlushed,
        ) {
            self.stream_read_fin_flushed.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_blocked(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadBlocked,
        ) {
            self.stream_read_blocked.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_errored(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadErrored,
        ) {
            self.stream_read_errored.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_key_updated(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadKeyUpdated,
        ) {
            self.stream_read_key_updated.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_shutdown(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadShutdown,
        ) {
            self.stream_read_shutdown.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_socket_flushed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketFlushed,
        ) {
            self.stream_read_socket_flushed
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_socket_blocked(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketBlocked,
        ) {
            self.stream_read_socket_blocked
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_socket_errored(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::StreamReadSocketErrored,
        ) {
            self.stream_read_socket_errored
                .fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_tcp_connect(&self, meta: &api::EndpointMeta, event: &api::StreamTcpConnect) {
            self.stream_tcp_connect.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stream_connect(&self, meta: &api::EndpointMeta, event: &api::StreamConnect) {
            self.stream_connect.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stream_connect_error(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StreamConnectError,
        ) {
            self.stream_connect_error.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_connection_closed(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ConnectionClosed,
        ) {
            self.connection_closed.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapInitialized,
        ) {
            self.path_secret_map_initialized
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_uninitialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapUninitialized,
        ) {
            self.path_secret_map_uninitialized
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_background_handshake_requested(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapBackgroundHandshakeRequested,
        ) {
            self.path_secret_map_background_handshake_requested
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_entry_inserted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryInserted,
        ) {
            self.path_secret_map_entry_inserted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_entry_ready(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReady,
        ) {
            self.path_secret_map_entry_ready
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_entry_replaced(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapEntryReplaced,
        ) {
            self.path_secret_map_entry_replaced
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_id_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdEntryEvicted,
        ) {
            self.path_secret_map_id_entry_evicted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_entry_evicted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressEntryEvicted,
        ) {
            self.path_secret_map_address_entry_evicted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketSent,
        ) {
            self.unknown_path_secret_packet_sent
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketReceived,
        ) {
            self.unknown_path_secret_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketAccepted,
        ) {
            self.unknown_path_secret_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketRejected,
        ) {
            self.unknown_path_secret_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::UnknownPathSecretPacketDropped,
        ) {
            self.unknown_path_secret_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_key_accepted(&self, meta: &api::EndpointMeta, event: &api::KeyAccepted) {
            self.key_accepted.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_definitely_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDefinitelyDetected,
        ) {
            self.replay_definitely_detected
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_potentially_detected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayPotentiallyDetected,
        ) {
            self.replay_potentially_detected
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketSent,
        ) {
            self.replay_detected_packet_sent
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketReceived,
        ) {
            self.replay_detected_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketAccepted,
        ) {
            self.replay_detected_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketRejected,
        ) {
            self.replay_detected_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::ReplayDetectedPacketDropped,
        ) {
            self.replay_detected_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketSent,
        ) {
            self.stale_key_packet_sent.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketReceived,
        ) {
            self.stale_key_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_accepted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketAccepted,
        ) {
            self.stale_key_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketRejected,
        ) {
            self.stale_key_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_dropped(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketDropped,
        ) {
            self.stale_key_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessed,
        ) {
            self.path_secret_map_address_cache_accessed
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressCacheAccessedHit,
        ) {
            self.path_secret_map_address_cache_accessed_hit
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_id_cache_accessed(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessed,
        ) {
            self.path_secret_map_id_cache_accessed
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdCacheAccessedHit,
        ) {
            self.path_secret_map_id_cache_accessed_hit
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_cleaner_cycled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapCleanerCycled,
        ) {
            self.path_secret_map_cleaner_cycled
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
    }
    #[derive(Debug)]
    pub struct Publisher {
        location: Option<Location>,
        output: Mutex<Vec<String>>,
        pub acceptor_tcp_started: AtomicU64,
        pub acceptor_tcp_loop_iteration_completed: AtomicU64,
        pub acceptor_tcp_fresh_enqueued: AtomicU64,
        pub acceptor_tcp_fresh_batch_completed: AtomicU64,
        pub acceptor_tcp_stream_dropped: AtomicU64,
        pub acceptor_tcp_stream_replaced: AtomicU64,
        pub acceptor_tcp_packet_received: AtomicU64,
        pub acceptor_tcp_packet_dropped: AtomicU64,
        pub acceptor_tcp_stream_enqueued: AtomicU64,
        pub acceptor_tcp_io_error: AtomicU64,
        pub acceptor_udp_started: AtomicU64,
        pub acceptor_udp_datagram_received: AtomicU64,
        pub acceptor_udp_packet_received: AtomicU64,
        pub acceptor_udp_packet_dropped: AtomicU64,
        pub acceptor_udp_stream_enqueued: AtomicU64,
        pub acceptor_udp_io_error: AtomicU64,
        pub acceptor_stream_pruned: AtomicU64,
        pub acceptor_stream_dequeued: AtomicU64,
        pub stream_write_flushed: AtomicU64,
        pub stream_write_fin_flushed: AtomicU64,
        pub stream_write_blocked: AtomicU64,
        pub stream_write_errored: AtomicU64,
        pub stream_write_key_updated: AtomicU64,
        pub stream_write_shutdown: AtomicU64,
        pub stream_write_socket_flushed: AtomicU64,
        pub stream_write_socket_blocked: AtomicU64,
        pub stream_write_socket_errored: AtomicU64,
        pub stream_read_flushed: AtomicU64,
        pub stream_read_fin_flushed: AtomicU64,
        pub stream_read_blocked: AtomicU64,
        pub stream_read_errored: AtomicU64,
        pub stream_read_key_updated: AtomicU64,
        pub stream_read_shutdown: AtomicU64,
        pub stream_read_socket_flushed: AtomicU64,
        pub stream_read_socket_blocked: AtomicU64,
        pub stream_read_socket_errored: AtomicU64,
        pub stream_tcp_connect: AtomicU64,
        pub stream_connect: AtomicU64,
        pub stream_connect_error: AtomicU64,
        pub connection_closed: AtomicU64,
        pub endpoint_initialized: AtomicU64,
        pub path_secret_map_initialized: AtomicU64,
        pub path_secret_map_uninitialized: AtomicU64,
        pub path_secret_map_background_handshake_requested: AtomicU64,
        pub path_secret_map_entry_inserted: AtomicU64,
        pub path_secret_map_entry_ready: AtomicU64,
        pub path_secret_map_entry_replaced: AtomicU64,
        pub path_secret_map_id_entry_evicted: AtomicU64,
        pub path_secret_map_address_entry_evicted: AtomicU64,
        pub unknown_path_secret_packet_sent: AtomicU64,
        pub unknown_path_secret_packet_received: AtomicU64,
        pub unknown_path_secret_packet_accepted: AtomicU64,
        pub unknown_path_secret_packet_rejected: AtomicU64,
        pub unknown_path_secret_packet_dropped: AtomicU64,
        pub key_accepted: AtomicU64,
        pub replay_definitely_detected: AtomicU64,
        pub replay_potentially_detected: AtomicU64,
        pub replay_detected_packet_sent: AtomicU64,
        pub replay_detected_packet_received: AtomicU64,
        pub replay_detected_packet_accepted: AtomicU64,
        pub replay_detected_packet_rejected: AtomicU64,
        pub replay_detected_packet_dropped: AtomicU64,
        pub stale_key_packet_sent: AtomicU64,
        pub stale_key_packet_received: AtomicU64,
        pub stale_key_packet_accepted: AtomicU64,
        pub stale_key_packet_rejected: AtomicU64,
        pub stale_key_packet_dropped: AtomicU64,
        pub path_secret_map_address_cache_accessed: AtomicU64,
        pub path_secret_map_address_cache_accessed_hit: AtomicU64,
        pub path_secret_map_id_cache_accessed: AtomicU64,
        pub path_secret_map_id_cache_accessed_hit: AtomicU64,
        pub path_secret_map_cleaner_cycled: AtomicU64,
    }
    impl Publisher {
        #[doc = r" Creates a publisher with snapshot assertions enabled"]
        #[track_caller]
        pub fn snapshot() -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Location::from_thread_name();
            sub
        }
        #[doc = r" Creates a subscriber with snapshot assertions enabled"]
        #[track_caller]
        pub fn named_snapshot<Name: core::fmt::Display>(name: Name) -> Self {
            let mut sub = Self::no_snapshot();
            sub.location = Some(Location::new(name));
            sub
        }
        #[doc = r" Creates a publisher with snapshot assertions disabled"]
        pub fn no_snapshot() -> Self {
            Self {
                location: None,
                output: Default::default(),
                acceptor_tcp_started: AtomicU64::new(0),
                acceptor_tcp_loop_iteration_completed: AtomicU64::new(0),
                acceptor_tcp_fresh_enqueued: AtomicU64::new(0),
                acceptor_tcp_fresh_batch_completed: AtomicU64::new(0),
                acceptor_tcp_stream_dropped: AtomicU64::new(0),
                acceptor_tcp_stream_replaced: AtomicU64::new(0),
                acceptor_tcp_packet_received: AtomicU64::new(0),
                acceptor_tcp_packet_dropped: AtomicU64::new(0),
                acceptor_tcp_stream_enqueued: AtomicU64::new(0),
                acceptor_tcp_io_error: AtomicU64::new(0),
                acceptor_udp_started: AtomicU64::new(0),
                acceptor_udp_datagram_received: AtomicU64::new(0),
                acceptor_udp_packet_received: AtomicU64::new(0),
                acceptor_udp_packet_dropped: AtomicU64::new(0),
                acceptor_udp_stream_enqueued: AtomicU64::new(0),
                acceptor_udp_io_error: AtomicU64::new(0),
                acceptor_stream_pruned: AtomicU64::new(0),
                acceptor_stream_dequeued: AtomicU64::new(0),
                stream_write_flushed: AtomicU64::new(0),
                stream_write_fin_flushed: AtomicU64::new(0),
                stream_write_blocked: AtomicU64::new(0),
                stream_write_errored: AtomicU64::new(0),
                stream_write_key_updated: AtomicU64::new(0),
                stream_write_shutdown: AtomicU64::new(0),
                stream_write_socket_flushed: AtomicU64::new(0),
                stream_write_socket_blocked: AtomicU64::new(0),
                stream_write_socket_errored: AtomicU64::new(0),
                stream_read_flushed: AtomicU64::new(0),
                stream_read_fin_flushed: AtomicU64::new(0),
                stream_read_blocked: AtomicU64::new(0),
                stream_read_errored: AtomicU64::new(0),
                stream_read_key_updated: AtomicU64::new(0),
                stream_read_shutdown: AtomicU64::new(0),
                stream_read_socket_flushed: AtomicU64::new(0),
                stream_read_socket_blocked: AtomicU64::new(0),
                stream_read_socket_errored: AtomicU64::new(0),
                stream_tcp_connect: AtomicU64::new(0),
                stream_connect: AtomicU64::new(0),
                stream_connect_error: AtomicU64::new(0),
                connection_closed: AtomicU64::new(0),
                endpoint_initialized: AtomicU64::new(0),
                path_secret_map_initialized: AtomicU64::new(0),
                path_secret_map_uninitialized: AtomicU64::new(0),
                path_secret_map_background_handshake_requested: AtomicU64::new(0),
                path_secret_map_entry_inserted: AtomicU64::new(0),
                path_secret_map_entry_ready: AtomicU64::new(0),
                path_secret_map_entry_replaced: AtomicU64::new(0),
                path_secret_map_id_entry_evicted: AtomicU64::new(0),
                path_secret_map_address_entry_evicted: AtomicU64::new(0),
                unknown_path_secret_packet_sent: AtomicU64::new(0),
                unknown_path_secret_packet_received: AtomicU64::new(0),
                unknown_path_secret_packet_accepted: AtomicU64::new(0),
                unknown_path_secret_packet_rejected: AtomicU64::new(0),
                unknown_path_secret_packet_dropped: AtomicU64::new(0),
                key_accepted: AtomicU64::new(0),
                replay_definitely_detected: AtomicU64::new(0),
                replay_potentially_detected: AtomicU64::new(0),
                replay_detected_packet_sent: AtomicU64::new(0),
                replay_detected_packet_received: AtomicU64::new(0),
                replay_detected_packet_accepted: AtomicU64::new(0),
                replay_detected_packet_rejected: AtomicU64::new(0),
                replay_detected_packet_dropped: AtomicU64::new(0),
                stale_key_packet_sent: AtomicU64::new(0),
                stale_key_packet_received: AtomicU64::new(0),
                stale_key_packet_accepted: AtomicU64::new(0),
                stale_key_packet_rejected: AtomicU64::new(0),
                stale_key_packet_dropped: AtomicU64::new(0),
                path_secret_map_address_cache_accessed: AtomicU64::new(0),
                path_secret_map_address_cache_accessed_hit: AtomicU64::new(0),
                path_secret_map_id_cache_accessed: AtomicU64::new(0),
                path_secret_map_id_cache_accessed_hit: AtomicU64::new(0),
                path_secret_map_cleaner_cycled: AtomicU64::new(0),
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn on_acceptor_tcp_started(&self, event: builder::AcceptorTcpStarted) {
            self.acceptor_tcp_started.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_loop_iteration_completed(
            &self,
            event: builder::AcceptorTcpLoopIterationCompleted,
        ) {
            self.acceptor_tcp_loop_iteration_completed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_fresh_enqueued(&self, event: builder::AcceptorTcpFreshEnqueued) {
            self.acceptor_tcp_fresh_enqueued
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_fresh_batch_completed(
            &self,
            event: builder::AcceptorTcpFreshBatchCompleted,
        ) {
            self.acceptor_tcp_fresh_batch_completed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_stream_dropped(&self, event: builder::AcceptorTcpStreamDropped) {
            self.acceptor_tcp_stream_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_stream_replaced(&self, event: builder::AcceptorTcpStreamReplaced) {
            self.acceptor_tcp_stream_replaced
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_packet_received(&self, event: builder::AcceptorTcpPacketReceived) {
            self.acceptor_tcp_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_packet_dropped(&self, event: builder::AcceptorTcpPacketDropped) {
            self.acceptor_tcp_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_stream_enqueued(&self, event: builder::AcceptorTcpStreamEnqueued) {
            self.acceptor_tcp_stream_enqueued
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_tcp_io_error(&self, event: builder::AcceptorTcpIoError) {
            self.acceptor_tcp_io_error.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_started(&self, event: builder::AcceptorUdpStarted) {
            self.acceptor_udp_started.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_datagram_received(&self, event: builder::AcceptorUdpDatagramReceived) {
            self.acceptor_udp_datagram_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_packet_received(&self, event: builder::AcceptorUdpPacketReceived) {
            self.acceptor_udp_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_packet_dropped(&self, event: builder::AcceptorUdpPacketDropped) {
            self.acceptor_udp_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_stream_enqueued(&self, event: builder::AcceptorUdpStreamEnqueued) {
            self.acceptor_udp_stream_enqueued
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_udp_io_error(&self, event: builder::AcceptorUdpIoError) {
            self.acceptor_udp_io_error.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_stream_pruned(&self, event: builder::AcceptorStreamPruned) {
            self.acceptor_stream_pruned.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_acceptor_stream_dequeued(&self, event: builder::AcceptorStreamDequeued) {
            self.acceptor_stream_dequeued
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stream_tcp_connect(&self, event: builder::StreamTcpConnect) {
            self.stream_tcp_connect.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stream_connect(&self, event: builder::StreamConnect) {
            self.stream_connect.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stream_connect_error(&self, event: builder::StreamConnectError) {
            self.stream_connect_error.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_initialized(&self, event: builder::PathSecretMapInitialized) {
            self.path_secret_map_initialized
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_uninitialized(&self, event: builder::PathSecretMapUninitialized) {
            self.path_secret_map_uninitialized
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_background_handshake_requested(
            &self,
            event: builder::PathSecretMapBackgroundHandshakeRequested,
        ) {
            self.path_secret_map_background_handshake_requested
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_entry_inserted(&self, event: builder::PathSecretMapEntryInserted) {
            self.path_secret_map_entry_inserted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_entry_ready(&self, event: builder::PathSecretMapEntryReady) {
            self.path_secret_map_entry_ready
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_entry_replaced(&self, event: builder::PathSecretMapEntryReplaced) {
            self.path_secret_map_entry_replaced
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_id_entry_evicted(&self, event: builder::PathSecretMapIdEntryEvicted) {
            self.path_secret_map_id_entry_evicted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_entry_evicted(
            &self,
            event: builder::PathSecretMapAddressEntryEvicted,
        ) {
            self.path_secret_map_address_entry_evicted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_sent(&self, event: builder::UnknownPathSecretPacketSent) {
            self.unknown_path_secret_packet_sent
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_received(
            &self,
            event: builder::UnknownPathSecretPacketReceived,
        ) {
            self.unknown_path_secret_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_accepted(
            &self,
            event: builder::UnknownPathSecretPacketAccepted,
        ) {
            self.unknown_path_secret_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_rejected(
            &self,
            event: builder::UnknownPathSecretPacketRejected,
        ) {
            self.unknown_path_secret_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_unknown_path_secret_packet_dropped(
            &self,
            event: builder::UnknownPathSecretPacketDropped,
        ) {
            self.unknown_path_secret_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_key_accepted(&self, event: builder::KeyAccepted) {
            self.key_accepted.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_definitely_detected(&self, event: builder::ReplayDefinitelyDetected) {
            self.replay_definitely_detected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_potentially_detected(&self, event: builder::ReplayPotentiallyDetected) {
            self.replay_potentially_detected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_sent(&self, event: builder::ReplayDetectedPacketSent) {
            self.replay_detected_packet_sent
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_received(&self, event: builder::ReplayDetectedPacketReceived) {
            self.replay_detected_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_accepted(&self, event: builder::ReplayDetectedPacketAccepted) {
            self.replay_detected_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_rejected(&self, event: builder::ReplayDetectedPacketRejected) {
            self.replay_detected_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_replay_detected_packet_dropped(&self, event: builder::ReplayDetectedPacketDropped) {
            self.replay_detected_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_sent(&self, event: builder::StaleKeyPacketSent) {
            self.stale_key_packet_sent.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_received(&self, event: builder::StaleKeyPacketReceived) {
            self.stale_key_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_accepted(&self, event: builder::StaleKeyPacketAccepted) {
            self.stale_key_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_rejected(&self, event: builder::StaleKeyPacketRejected) {
            self.stale_key_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_stale_key_packet_dropped(&self, event: builder::StaleKeyPacketDropped) {
            self.stale_key_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_cache_accessed(
            &self,
            event: builder::PathSecretMapAddressCacheAccessed,
        ) {
            self.path_secret_map_address_cache_accessed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_cache_accessed_hit(
            &self,
            event: builder::PathSecretMapAddressCacheAccessedHit,
        ) {
            self.path_secret_map_address_cache_accessed_hit
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_id_cache_accessed(
            &self,
            event: builder::PathSecretMapIdCacheAccessed,
        ) {
            self.path_secret_map_id_cache_accessed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_id_cache_accessed_hit(
            &self,
            event: builder::PathSecretMapIdCacheAccessedHit,
        ) {
            self.path_secret_map_id_cache_accessed_hit
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_cleaner_cycled(&self, event: builder::PathSecretMapCleanerCycled) {
            self.path_secret_map_cleaner_cycled
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_stream_write_flushed(&self, event: builder::StreamWriteFlushed) {
            self.stream_write_flushed.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_fin_flushed(&self, event: builder::StreamWriteFinFlushed) {
            self.stream_write_fin_flushed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_blocked(&self, event: builder::StreamWriteBlocked) {
            self.stream_write_blocked.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_errored(&self, event: builder::StreamWriteErrored) {
            self.stream_write_errored.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_key_updated(&self, event: builder::StreamWriteKeyUpdated) {
            self.stream_write_key_updated
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_shutdown(&self, event: builder::StreamWriteShutdown) {
            self.stream_write_shutdown.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_socket_flushed(&self, event: builder::StreamWriteSocketFlushed) {
            self.stream_write_socket_flushed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_socket_blocked(&self, event: builder::StreamWriteSocketBlocked) {
            self.stream_write_socket_blocked
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_write_socket_errored(&self, event: builder::StreamWriteSocketErrored) {
            self.stream_write_socket_errored
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_flushed(&self, event: builder::StreamReadFlushed) {
            self.stream_read_flushed.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_fin_flushed(&self, event: builder::StreamReadFinFlushed) {
            self.stream_read_fin_flushed.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_blocked(&self, event: builder::StreamReadBlocked) {
            self.stream_read_blocked.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_errored(&self, event: builder::StreamReadErrored) {
            self.stream_read_errored.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_key_updated(&self, event: builder::StreamReadKeyUpdated) {
            self.stream_read_key_updated.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_shutdown(&self, event: builder::StreamReadShutdown) {
            self.stream_read_shutdown.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_socket_flushed(&self, event: builder::StreamReadSocketFlushed) {
            self.stream_read_socket_flushed
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_socket_blocked(&self, event: builder::StreamReadSocketBlocked) {
            self.stream_read_socket_blocked
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_stream_read_socket_errored(&self, event: builder::StreamReadSocketErrored) {
            self.stream_read_socket_errored
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn on_connection_closed(&self, event: builder::ConnectionClosed) {
            self.connection_closed.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                let out = format!("{event:?}");
                self.output.lock().unwrap().push(out);
            }
        }
        fn quic_version(&self) -> u32 {
            1
        }
        fn subject(&self) -> api::Subject {
            builder::Subject::Connection { id: 0 }.into_event()
        }
    }
    impl Drop for Publisher {
        fn drop(&mut self) {
            if std::thread::panicking() {
                return;
            }
            if let Some(location) = self.location.as_ref() {
                location.snapshot_log(&self.output.lock().unwrap());
            }
        }
    }
}

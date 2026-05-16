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
    pub struct ConnectionInfo<'a> {
        #[doc = " The credential ID (path secret identifier) for this stream"]
        pub credential_id: &'a [u8],
        #[doc = " The key ID (per-stream counter derived from the path secret)"]
        pub key_id: u64,
        #[doc = " The remote peer address"]
        pub remote_address: SocketAddress<'a>,
        #[doc = " Whether this is the client or server side of the stream"]
        pub is_server: bool,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for ConnectionInfo<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionInfo");
            fmt.field("credential_id", &"[HIDDEN]");
            fmt.field("key_id", &self.key_id);
            fmt.field("remote_address", &self.remote_address);
            fmt.field("is_server", &self.is_server);
            fmt.finish()
        }
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
    #[doc = " Emitted when the DC handshake confirmation or MTU probing times out"]
    pub struct DcConnectionTimeout<'a> {
        pub peer_address: SocketAddress<'a>,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for DcConnectionTimeout<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("DcConnectionTimeout");
            fmt.field("peer_address", &self.peer_address);
            fmt.finish()
        }
    }
    impl<'a> Event for DcConnectionTimeout<'a> {
        const NAME: &'static str = "dc:connection_timeout";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Called when a transmission is scheduled for immediate transmission"]
    pub struct EndpointUdpImmediateTransmissionScheduled<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpImmediateTransmissionScheduled<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpImmediateTransmissionScheduled");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("buffer_size", &self.buffer_size);
            fmt.field("segment_size", &self.segment_size);
            fmt.field("segment_count", &self.segment_count);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpImmediateTransmissionScheduled<'a> {
        const NAME: &'static str = "endpoint:udp:immediate_transmission_scheduled";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Called when a transmission is scheduled in the future"]
    pub struct EndpointUdpTransmissionScheduled<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
        pub delay: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpTransmissionScheduled<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpTransmissionScheduled");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("buffer_size", &self.buffer_size);
            fmt.field("segment_size", &self.segment_size);
            fmt.field("segment_count", &self.segment_count);
            fmt.field("delay", &self.delay);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpTransmissionScheduled<'a> {
        const NAME: &'static str = "endpoint:udp:transmission_scheduled";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    #[doc = " Called when a transmission is rejected"]
    pub struct EndpointUdpTransmissionRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
        pub delay: core::time::Duration,
        pub backoff: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpTransmissionRejected<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpTransmissionRejected");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("buffer_size", &self.buffer_size);
            fmt.field("segment_size", &self.segment_size);
            fmt.field("segment_count", &self.segment_count);
            fmt.field("delay", &self.delay);
            fmt.field("backoff", &self.backoff);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpTransmissionRejected<'a> {
        const NAME: &'static str = "endpoint:udp:transmission_rejected";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointUdpPacketTransmitted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpPacketTransmitted<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpPacketTransmitted");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("buffer_size", &self.buffer_size);
            fmt.field("segment_size", &self.segment_size);
            fmt.field("segment_count", &self.segment_count);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpPacketTransmitted<'a> {
        const NAME: &'static str = "endpoint:udp:packet_transmitted";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointUdpTransmitErrored<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
        pub error: &'a std::io::Error,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpTransmitErrored<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpTransmitErrored");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("buffer_size", &self.buffer_size);
            fmt.field("segment_size", &self.segment_size);
            fmt.field("segment_count", &self.segment_count);
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpTransmitErrored<'a> {
        const NAME: &'static str = "endpoint:udp:transmit_errored";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointUdpPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpPacketReceived<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpPacketReceived");
            fmt.field("peer_address", &self.peer_address);
            fmt.field("buffer_size", &self.buffer_size);
            fmt.field("segment_size", &self.segment_size);
            fmt.field("segment_count", &self.segment_count);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpPacketReceived<'a> {
        const NAME: &'static str = "endpoint:udp:packet_received";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointUdpReceiveErrored<'a> {
        pub error: &'a std::io::Error,
    }
    #[cfg(any(test, feature = "testing"))]
    impl<'a> crate::event::snapshot::Fmt for EndpointUdpReceiveErrored<'a> {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointUdpReceiveErrored");
            fmt.field("error", &self.error);
            fmt.finish()
        }
    }
    impl<'a> Event for EndpointUdpReceiveErrored<'a> {
        const NAME: &'static str = "endpoint:udp:receive_errored";
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
        #[doc = " The number of handshake requests that were skipped in the cycle due to running out of time"]
        #[doc = " (other background handshakes took too long to complete, and so were postponed to the next"]
        #[doc = " cleaner cycle)."]
        pub handshake_requests_skipped: usize,
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
                "handshake_requests_skipped",
                &self.handshake_requests_skipped,
            );
            fmt.field("handshake_lock_duration", &self.handshake_lock_duration);
            fmt.field("duration", &self.duration);
            fmt.finish()
        }
    }
    impl Event for PathSecretMapCleanerCycled {
        const NAME: &'static str = "path_secret_map:cleaner_cycled";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PathSecretMapIdWriteLock {
        pub acquire: core::time::Duration,
        pub duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PathSecretMapIdWriteLock {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapIdWriteLock");
            fmt.field("acquire", &self.acquire);
            fmt.field("duration", &self.duration);
            fmt.finish()
        }
    }
    impl Event for PathSecretMapIdWriteLock {
        const NAME: &'static str = "path_secret_map:id_cache_write_lock";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct PathSecretMapAddressWriteLock {
        pub acquire: core::time::Duration,
        pub duration: core::time::Duration,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PathSecretMapAddressWriteLock {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapAddressWriteLock");
            fmt.field("acquire", &self.acquire);
            fmt.field("duration", &self.duration);
            fmt.finish()
        }
    }
    impl Event for PathSecretMapAddressWriteLock {
        const NAME: &'static str = "path_secret_map:address_cache_write_lock";
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
        fn on_dc_connection_timeout(
            &self,
            meta: &api::EndpointMeta,
            event: &api::DcConnectionTimeout,
        ) {
            let parent = self.parent(meta);
            let api::DcConnectionTimeout { peer_address } = event;
            tracing :: event ! (target : "dc_connection_timeout" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) });
        }
        #[inline]
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpImmediateTransmissionScheduled,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpImmediateTransmissionScheduled {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
            } = event;
            tracing :: event ! (target : "endpoint_udp_immediate_transmission_scheduled" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , buffer_size = tracing :: field :: debug (buffer_size) , segment_size = tracing :: field :: debug (segment_size) , segment_count = tracing :: field :: debug (segment_count) });
        }
        #[inline]
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionScheduled,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpTransmissionScheduled {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
                delay,
            } = event;
            tracing :: event ! (target : "endpoint_udp_transmission_scheduled" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , buffer_size = tracing :: field :: debug (buffer_size) , segment_size = tracing :: field :: debug (segment_size) , segment_count = tracing :: field :: debug (segment_count) , delay = tracing :: field :: debug (delay) });
        }
        #[inline]
        fn on_endpoint_udp_transmission_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionRejected,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpTransmissionRejected {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
                delay,
                backoff,
            } = event;
            tracing :: event ! (target : "endpoint_udp_transmission_rejected" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , buffer_size = tracing :: field :: debug (buffer_size) , segment_size = tracing :: field :: debug (segment_size) , segment_count = tracing :: field :: debug (segment_count) , delay = tracing :: field :: debug (delay) , backoff = tracing :: field :: debug (backoff) });
        }
        #[inline]
        fn on_endpoint_udp_packet_transmitted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketTransmitted,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpPacketTransmitted {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
            } = event;
            tracing :: event ! (target : "endpoint_udp_packet_transmitted" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , buffer_size = tracing :: field :: debug (buffer_size) , segment_size = tracing :: field :: debug (segment_size) , segment_count = tracing :: field :: debug (segment_count) });
        }
        #[inline]
        fn on_endpoint_udp_transmit_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmitErrored,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpTransmitErrored {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
                error,
            } = event;
            tracing :: event ! (target : "endpoint_udp_transmit_errored" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , buffer_size = tracing :: field :: debug (buffer_size) , segment_size = tracing :: field :: debug (segment_size) , segment_count = tracing :: field :: debug (segment_count) , error = tracing :: field :: debug (error) });
        }
        #[inline]
        fn on_endpoint_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketReceived,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpPacketReceived {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
            } = event;
            tracing :: event ! (target : "endpoint_udp_packet_received" , parent : parent , tracing :: Level :: DEBUG , { peer_address = tracing :: field :: debug (peer_address) , buffer_size = tracing :: field :: debug (buffer_size) , segment_size = tracing :: field :: debug (segment_size) , segment_count = tracing :: field :: debug (segment_count) });
        }
        #[inline]
        fn on_endpoint_udp_receive_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpReceiveErrored,
        ) {
            let parent = self.parent(meta);
            let api::EndpointUdpReceiveErrored { error } = event;
            tracing :: event ! (target : "endpoint_udp_receive_errored" , parent : parent , tracing :: Level :: DEBUG , { error = tracing :: field :: debug (error) });
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
                handshake_requests_skipped,
                handshake_lock_duration,
                duration,
            } = event;
            tracing :: event ! (target : "path_secret_map_cleaner_cycled" , parent : parent , tracing :: Level :: DEBUG , { id_entries = tracing :: field :: debug (id_entries) , id_entries_retired = tracing :: field :: debug (id_entries_retired) , id_entries_active = tracing :: field :: debug (id_entries_active) , id_entries_active_utilization = tracing :: field :: debug (id_entries_active_utilization) , id_entries_utilization = tracing :: field :: debug (id_entries_utilization) , id_entries_initial_utilization = tracing :: field :: debug (id_entries_initial_utilization) , address_entries = tracing :: field :: debug (address_entries) , address_entries_active = tracing :: field :: debug (address_entries_active) , address_entries_active_utilization = tracing :: field :: debug (address_entries_active_utilization) , address_entries_retired = tracing :: field :: debug (address_entries_retired) , address_entries_utilization = tracing :: field :: debug (address_entries_utilization) , address_entries_initial_utilization = tracing :: field :: debug (address_entries_initial_utilization) , handshake_requests = tracing :: field :: debug (handshake_requests) , handshake_requests_skipped = tracing :: field :: debug (handshake_requests_skipped) , handshake_lock_duration = tracing :: field :: debug (handshake_lock_duration) , duration = tracing :: field :: debug (duration) });
        }
        #[inline]
        fn on_path_secret_map_id_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdWriteLock,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapIdWriteLock { acquire, duration } = event;
            tracing :: event ! (target : "path_secret_map_id_write_lock" , parent : parent , tracing :: Level :: DEBUG , { acquire = tracing :: field :: debug (acquire) , duration = tracing :: field :: debug (duration) });
        }
        #[inline]
        fn on_path_secret_map_address_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressWriteLock,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapAddressWriteLock { acquire, duration } = event;
            tracing :: event ! (target : "path_secret_map_address_write_lock" , parent : parent , tracing :: Level :: DEBUG , { acquire = tracing :: field :: debug (acquire) , duration = tracing :: field :: debug (duration) });
        }
    }
}
pub mod builder {
    use super::*;
    pub use s2n_quic_core::event::builder::{EndpointType, SocketAddress, Subject};
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
    pub struct ConnectionInfo<'a> {
        #[doc = " The credential ID (path secret identifier) for this stream"]
        pub credential_id: &'a [u8],
        #[doc = " The key ID (per-stream counter derived from the path secret)"]
        pub key_id: u64,
        #[doc = " The remote peer address"]
        pub remote_address: &'a s2n_quic_core::inet::SocketAddress,
        #[doc = " Whether this is the client or server side of the stream"]
        pub is_server: bool,
    }
    impl<'a> IntoEvent<api::ConnectionInfo<'a>> for ConnectionInfo<'a> {
        #[inline]
        fn into_event(self) -> api::ConnectionInfo<'a> {
            let ConnectionInfo {
                credential_id,
                key_id,
                remote_address,
                is_server,
            } = self;
            api::ConnectionInfo {
                credential_id: credential_id.into_event(),
                key_id: key_id.into_event(),
                remote_address: remote_address.into_event(),
                is_server: is_server.into_event(),
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
    #[doc = " Emitted when the DC handshake confirmation or MTU probing times out"]
    pub struct DcConnectionTimeout<'a> {
        pub peer_address: SocketAddress<'a>,
    }
    impl<'a> IntoEvent<api::DcConnectionTimeout<'a>> for DcConnectionTimeout<'a> {
        #[inline]
        fn into_event(self) -> api::DcConnectionTimeout<'a> {
            let DcConnectionTimeout { peer_address } = self;
            api::DcConnectionTimeout {
                peer_address: peer_address.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Called when a transmission is scheduled for immediate transmission"]
    pub struct EndpointUdpImmediateTransmissionScheduled<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
    }
    impl<'a> IntoEvent<api::EndpointUdpImmediateTransmissionScheduled<'a>>
        for EndpointUdpImmediateTransmissionScheduled<'a>
    {
        #[inline]
        fn into_event(self) -> api::EndpointUdpImmediateTransmissionScheduled<'a> {
            let EndpointUdpImmediateTransmissionScheduled {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
            } = self;
            api::EndpointUdpImmediateTransmissionScheduled {
                peer_address: peer_address.into_event(),
                buffer_size: buffer_size.into_event(),
                segment_size: segment_size.into_event(),
                segment_count: segment_count.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Called when a transmission is scheduled in the future"]
    pub struct EndpointUdpTransmissionScheduled<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
        pub delay: core::time::Duration,
    }
    impl<'a> IntoEvent<api::EndpointUdpTransmissionScheduled<'a>>
        for EndpointUdpTransmissionScheduled<'a>
    {
        #[inline]
        fn into_event(self) -> api::EndpointUdpTransmissionScheduled<'a> {
            let EndpointUdpTransmissionScheduled {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
                delay,
            } = self;
            api::EndpointUdpTransmissionScheduled {
                peer_address: peer_address.into_event(),
                buffer_size: buffer_size.into_event(),
                segment_size: segment_size.into_event(),
                segment_count: segment_count.into_event(),
                delay: delay.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    #[doc = " Called when a transmission is rejected"]
    pub struct EndpointUdpTransmissionRejected<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
        pub delay: core::time::Duration,
        pub backoff: core::time::Duration,
    }
    impl<'a> IntoEvent<api::EndpointUdpTransmissionRejected<'a>>
        for EndpointUdpTransmissionRejected<'a>
    {
        #[inline]
        fn into_event(self) -> api::EndpointUdpTransmissionRejected<'a> {
            let EndpointUdpTransmissionRejected {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
                delay,
                backoff,
            } = self;
            api::EndpointUdpTransmissionRejected {
                peer_address: peer_address.into_event(),
                buffer_size: buffer_size.into_event(),
                segment_size: segment_size.into_event(),
                segment_count: segment_count.into_event(),
                delay: delay.into_event(),
                backoff: backoff.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointUdpPacketTransmitted<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
    }
    impl<'a> IntoEvent<api::EndpointUdpPacketTransmitted<'a>> for EndpointUdpPacketTransmitted<'a> {
        #[inline]
        fn into_event(self) -> api::EndpointUdpPacketTransmitted<'a> {
            let EndpointUdpPacketTransmitted {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
            } = self;
            api::EndpointUdpPacketTransmitted {
                peer_address: peer_address.into_event(),
                buffer_size: buffer_size.into_event(),
                segment_size: segment_size.into_event(),
                segment_count: segment_count.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointUdpTransmitErrored<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
        pub error: &'a std::io::Error,
    }
    impl<'a> IntoEvent<api::EndpointUdpTransmitErrored<'a>> for EndpointUdpTransmitErrored<'a> {
        #[inline]
        fn into_event(self) -> api::EndpointUdpTransmitErrored<'a> {
            let EndpointUdpTransmitErrored {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
                error,
            } = self;
            api::EndpointUdpTransmitErrored {
                peer_address: peer_address.into_event(),
                buffer_size: buffer_size.into_event(),
                segment_size: segment_size.into_event(),
                segment_count: segment_count.into_event(),
                error: error.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointUdpPacketReceived<'a> {
        pub peer_address: SocketAddress<'a>,
        pub buffer_size: u16,
        pub segment_size: u16,
        pub segment_count: u16,
    }
    impl<'a> IntoEvent<api::EndpointUdpPacketReceived<'a>> for EndpointUdpPacketReceived<'a> {
        #[inline]
        fn into_event(self) -> api::EndpointUdpPacketReceived<'a> {
            let EndpointUdpPacketReceived {
                peer_address,
                buffer_size,
                segment_size,
                segment_count,
            } = self;
            api::EndpointUdpPacketReceived {
                peer_address: peer_address.into_event(),
                buffer_size: buffer_size.into_event(),
                segment_size: segment_size.into_event(),
                segment_count: segment_count.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointUdpReceiveErrored<'a> {
        pub error: &'a std::io::Error,
    }
    impl<'a> IntoEvent<api::EndpointUdpReceiveErrored<'a>> for EndpointUdpReceiveErrored<'a> {
        #[inline]
        fn into_event(self) -> api::EndpointUdpReceiveErrored<'a> {
            let EndpointUdpReceiveErrored { error } = self;
            api::EndpointUdpReceiveErrored {
                error: error.into_event(),
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
        #[doc = " The number of handshake requests that were skipped in the cycle due to running out of time"]
        #[doc = " (other background handshakes took too long to complete, and so were postponed to the next"]
        #[doc = " cleaner cycle)."]
        pub handshake_requests_skipped: usize,
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
                handshake_requests_skipped,
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
                handshake_requests_skipped: handshake_requests_skipped.into_event(),
                handshake_lock_duration: handshake_lock_duration.into_event(),
                duration: duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PathSecretMapIdWriteLock {
        pub acquire: core::time::Duration,
        pub duration: core::time::Duration,
    }
    impl IntoEvent<api::PathSecretMapIdWriteLock> for PathSecretMapIdWriteLock {
        #[inline]
        fn into_event(self) -> api::PathSecretMapIdWriteLock {
            let PathSecretMapIdWriteLock { acquire, duration } = self;
            api::PathSecretMapIdWriteLock {
                acquire: acquire.into_event(),
                duration: duration.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct PathSecretMapAddressWriteLock {
        pub acquire: core::time::Duration,
        pub duration: core::time::Duration,
    }
    impl IntoEvent<api::PathSecretMapAddressWriteLock> for PathSecretMapAddressWriteLock {
        #[inline]
        fn into_event(self) -> api::PathSecretMapAddressWriteLock {
            let PathSecretMapAddressWriteLock { acquire, duration } = self;
            api::PathSecretMapAddressWriteLock {
                acquire: acquire.into_event(),
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
        #[doc = "Called when the `DcConnectionTimeout` event is triggered"]
        #[inline]
        fn on_dc_connection_timeout(
            &self,
            meta: &api::EndpointMeta,
            event: &api::DcConnectionTimeout,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpImmediateTransmissionScheduled` event is triggered"]
        #[inline]
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpImmediateTransmissionScheduled,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpTransmissionScheduled` event is triggered"]
        #[inline]
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionScheduled,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpTransmissionRejected` event is triggered"]
        #[inline]
        fn on_endpoint_udp_transmission_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionRejected,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpPacketTransmitted` event is triggered"]
        #[inline]
        fn on_endpoint_udp_packet_transmitted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketTransmitted,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpTransmitErrored` event is triggered"]
        #[inline]
        fn on_endpoint_udp_transmit_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmitErrored,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpPacketReceived` event is triggered"]
        #[inline]
        fn on_endpoint_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketReceived,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `EndpointUdpReceiveErrored` event is triggered"]
        #[inline]
        fn on_endpoint_udp_receive_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpReceiveErrored,
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
        #[doc = "Called when the `PathSecretMapIdWriteLock` event is triggered"]
        #[inline]
        fn on_path_secret_map_id_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdWriteLock,
        ) {
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `PathSecretMapAddressWriteLock` event is triggered"]
        #[inline]
        fn on_path_secret_map_address_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressWriteLock,
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
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            self.as_ref().on_endpoint_initialized(meta, event);
        }
        #[inline]
        fn on_dc_connection_timeout(
            &self,
            meta: &api::EndpointMeta,
            event: &api::DcConnectionTimeout,
        ) {
            self.as_ref().on_dc_connection_timeout(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpImmediateTransmissionScheduled,
        ) {
            self.as_ref()
                .on_endpoint_udp_immediate_transmission_scheduled(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionScheduled,
        ) {
            self.as_ref()
                .on_endpoint_udp_transmission_scheduled(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_transmission_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionRejected,
        ) {
            self.as_ref()
                .on_endpoint_udp_transmission_rejected(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_packet_transmitted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketTransmitted,
        ) {
            self.as_ref()
                .on_endpoint_udp_packet_transmitted(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_transmit_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmitErrored,
        ) {
            self.as_ref().on_endpoint_udp_transmit_errored(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketReceived,
        ) {
            self.as_ref().on_endpoint_udp_packet_received(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_receive_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpReceiveErrored,
        ) {
            self.as_ref().on_endpoint_udp_receive_errored(meta, event);
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
        fn on_path_secret_map_id_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdWriteLock,
        ) {
            self.as_ref().on_path_secret_map_id_write_lock(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressWriteLock,
        ) {
            self.as_ref()
                .on_path_secret_map_address_write_lock(meta, event);
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
        fn on_endpoint_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointInitialized,
        ) {
            (self.0).on_endpoint_initialized(meta, event);
            (self.1).on_endpoint_initialized(meta, event);
        }
        #[inline]
        fn on_dc_connection_timeout(
            &self,
            meta: &api::EndpointMeta,
            event: &api::DcConnectionTimeout,
        ) {
            (self.0).on_dc_connection_timeout(meta, event);
            (self.1).on_dc_connection_timeout(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpImmediateTransmissionScheduled,
        ) {
            (self.0).on_endpoint_udp_immediate_transmission_scheduled(meta, event);
            (self.1).on_endpoint_udp_immediate_transmission_scheduled(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionScheduled,
        ) {
            (self.0).on_endpoint_udp_transmission_scheduled(meta, event);
            (self.1).on_endpoint_udp_transmission_scheduled(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_transmission_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionRejected,
        ) {
            (self.0).on_endpoint_udp_transmission_rejected(meta, event);
            (self.1).on_endpoint_udp_transmission_rejected(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_packet_transmitted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketTransmitted,
        ) {
            (self.0).on_endpoint_udp_packet_transmitted(meta, event);
            (self.1).on_endpoint_udp_packet_transmitted(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_transmit_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmitErrored,
        ) {
            (self.0).on_endpoint_udp_transmit_errored(meta, event);
            (self.1).on_endpoint_udp_transmit_errored(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketReceived,
        ) {
            (self.0).on_endpoint_udp_packet_received(meta, event);
            (self.1).on_endpoint_udp_packet_received(meta, event);
        }
        #[inline]
        fn on_endpoint_udp_receive_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpReceiveErrored,
        ) {
            (self.0).on_endpoint_udp_receive_errored(meta, event);
            (self.1).on_endpoint_udp_receive_errored(meta, event);
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
        fn on_path_secret_map_id_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdWriteLock,
        ) {
            (self.0).on_path_secret_map_id_write_lock(meta, event);
            (self.1).on_path_secret_map_id_write_lock(meta, event);
        }
        #[inline]
        fn on_path_secret_map_address_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressWriteLock,
        ) {
            (self.0).on_path_secret_map_address_write_lock(meta, event);
            (self.1).on_path_secret_map_address_write_lock(meta, event);
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
        #[doc = "Publishes a `EndpointInitialized` event to the publisher's subscriber"]
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized);
        #[doc = "Publishes a `DcConnectionTimeout` event to the publisher's subscriber"]
        fn on_dc_connection_timeout(&self, event: builder::DcConnectionTimeout);
        #[doc = "Publishes a `EndpointUdpImmediateTransmissionScheduled` event to the publisher's subscriber"]
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            event: builder::EndpointUdpImmediateTransmissionScheduled,
        );
        #[doc = "Publishes a `EndpointUdpTransmissionScheduled` event to the publisher's subscriber"]
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            event: builder::EndpointUdpTransmissionScheduled,
        );
        #[doc = "Publishes a `EndpointUdpTransmissionRejected` event to the publisher's subscriber"]
        fn on_endpoint_udp_transmission_rejected(
            &self,
            event: builder::EndpointUdpTransmissionRejected,
        );
        #[doc = "Publishes a `EndpointUdpPacketTransmitted` event to the publisher's subscriber"]
        fn on_endpoint_udp_packet_transmitted(&self, event: builder::EndpointUdpPacketTransmitted);
        #[doc = "Publishes a `EndpointUdpTransmitErrored` event to the publisher's subscriber"]
        fn on_endpoint_udp_transmit_errored(&self, event: builder::EndpointUdpTransmitErrored);
        #[doc = "Publishes a `EndpointUdpPacketReceived` event to the publisher's subscriber"]
        fn on_endpoint_udp_packet_received(&self, event: builder::EndpointUdpPacketReceived);
        #[doc = "Publishes a `EndpointUdpReceiveErrored` event to the publisher's subscriber"]
        fn on_endpoint_udp_receive_errored(&self, event: builder::EndpointUdpReceiveErrored);
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
        #[doc = "Publishes a `PathSecretMapIdWriteLock` event to the publisher's subscriber"]
        fn on_path_secret_map_id_write_lock(&self, event: builder::PathSecretMapIdWriteLock);
        #[doc = "Publishes a `PathSecretMapAddressWriteLock` event to the publisher's subscriber"]
        fn on_path_secret_map_address_write_lock(
            &self,
            event: builder::PathSecretMapAddressWriteLock,
        );
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
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            let event = event.into_event();
            self.subscriber.on_endpoint_initialized(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_dc_connection_timeout(&self, event: builder::DcConnectionTimeout) {
            let event = event.into_event();
            self.subscriber.on_dc_connection_timeout(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            event: builder::EndpointUdpImmediateTransmissionScheduled,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_immediate_transmission_scheduled(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            event: builder::EndpointUdpTransmissionScheduled,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_transmission_scheduled(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_transmission_rejected(
            &self,
            event: builder::EndpointUdpTransmissionRejected,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_transmission_rejected(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_packet_transmitted(&self, event: builder::EndpointUdpPacketTransmitted) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_packet_transmitted(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_transmit_errored(&self, event: builder::EndpointUdpTransmitErrored) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_transmit_errored(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_packet_received(&self, event: builder::EndpointUdpPacketReceived) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_packet_received(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_endpoint_udp_receive_errored(&self, event: builder::EndpointUdpReceiveErrored) {
            let event = event.into_event();
            self.subscriber
                .on_endpoint_udp_receive_errored(&self.meta, &event);
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
        fn on_path_secret_map_id_write_lock(&self, event: builder::PathSecretMapIdWriteLock) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_id_write_lock(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_path_secret_map_address_write_lock(
            &self,
            event: builder::PathSecretMapAddressWriteLock,
        ) {
            let event = event.into_event();
            self.subscriber
                .on_path_secret_map_address_write_lock(&self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `StreamWriteKeyUpdated` event to the publisher's subscriber"]
        fn on_stream_write_key_updated(&self, event: builder::StreamWriteKeyUpdated);
        #[doc = "Publishes a `StreamReadKeyUpdated` event to the publisher's subscriber"]
        fn on_stream_read_key_updated(&self, event: builder::StreamReadKeyUpdated);
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
        fn on_stream_write_key_updated(&self, event: builder::StreamWriteKeyUpdated) {
            let event = event.into_event();
            self.subscriber
                .on_stream_write_key_updated(self.context, &self.meta, &event);
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
    #[allow(unused_imports)]
    use core::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    pub mod endpoint {
        use super::*;
        pub struct Subscriber {
            location: Option<Location>,
            output: Mutex<Vec<String>>,
            pub endpoint_initialized: AtomicU64,
            pub dc_connection_timeout: AtomicU64,
            pub endpoint_udp_immediate_transmission_scheduled: AtomicU64,
            pub endpoint_udp_transmission_scheduled: AtomicU64,
            pub endpoint_udp_transmission_rejected: AtomicU64,
            pub endpoint_udp_packet_transmitted: AtomicU64,
            pub endpoint_udp_transmit_errored: AtomicU64,
            pub endpoint_udp_packet_received: AtomicU64,
            pub endpoint_udp_receive_errored: AtomicU64,
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
            pub path_secret_map_id_write_lock: AtomicU64,
            pub path_secret_map_address_write_lock: AtomicU64,
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
                    endpoint_initialized: AtomicU64::new(0),
                    dc_connection_timeout: AtomicU64::new(0),
                    endpoint_udp_immediate_transmission_scheduled: AtomicU64::new(0),
                    endpoint_udp_transmission_scheduled: AtomicU64::new(0),
                    endpoint_udp_transmission_rejected: AtomicU64::new(0),
                    endpoint_udp_packet_transmitted: AtomicU64::new(0),
                    endpoint_udp_transmit_errored: AtomicU64::new(0),
                    endpoint_udp_packet_received: AtomicU64::new(0),
                    endpoint_udp_receive_errored: AtomicU64::new(0),
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
                    path_secret_map_id_write_lock: AtomicU64::new(0),
                    path_secret_map_address_write_lock: AtomicU64::new(0),
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
            fn on_dc_connection_timeout(
                &self,
                meta: &api::EndpointMeta,
                event: &api::DcConnectionTimeout,
            ) {
                self.dc_connection_timeout.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_immediate_transmission_scheduled(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpImmediateTransmissionScheduled,
            ) {
                self.endpoint_udp_immediate_transmission_scheduled
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_transmission_scheduled(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpTransmissionScheduled,
            ) {
                self.endpoint_udp_transmission_scheduled
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_transmission_rejected(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpTransmissionRejected,
            ) {
                self.endpoint_udp_transmission_rejected
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_packet_transmitted(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpPacketTransmitted,
            ) {
                self.endpoint_udp_packet_transmitted
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_transmit_errored(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpTransmitErrored,
            ) {
                self.endpoint_udp_transmit_errored
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_packet_received(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpPacketReceived,
            ) {
                self.endpoint_udp_packet_received
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_endpoint_udp_receive_errored(
                &self,
                meta: &api::EndpointMeta,
                event: &api::EndpointUdpReceiveErrored,
            ) {
                self.endpoint_udp_receive_errored
                    .fetch_add(1, Ordering::Relaxed);
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
            fn on_path_secret_map_id_write_lock(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapIdWriteLock,
            ) {
                self.path_secret_map_id_write_lock
                    .fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                let out = format!("{meta:?} {event:?}");
                self.output.lock().unwrap().push(out);
            }
            fn on_path_secret_map_address_write_lock(
                &self,
                meta: &api::EndpointMeta,
                event: &api::PathSecretMapAddressWriteLock,
            ) {
                self.path_secret_map_address_write_lock
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
        pub stream_write_key_updated: AtomicU64,
        pub stream_read_key_updated: AtomicU64,
        pub endpoint_initialized: AtomicU64,
        pub dc_connection_timeout: AtomicU64,
        pub endpoint_udp_immediate_transmission_scheduled: AtomicU64,
        pub endpoint_udp_transmission_scheduled: AtomicU64,
        pub endpoint_udp_transmission_rejected: AtomicU64,
        pub endpoint_udp_packet_transmitted: AtomicU64,
        pub endpoint_udp_transmit_errored: AtomicU64,
        pub endpoint_udp_packet_received: AtomicU64,
        pub endpoint_udp_receive_errored: AtomicU64,
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
        pub path_secret_map_id_write_lock: AtomicU64,
        pub path_secret_map_address_write_lock: AtomicU64,
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
                stream_write_key_updated: AtomicU64::new(0),
                stream_read_key_updated: AtomicU64::new(0),
                endpoint_initialized: AtomicU64::new(0),
                dc_connection_timeout: AtomicU64::new(0),
                endpoint_udp_immediate_transmission_scheduled: AtomicU64::new(0),
                endpoint_udp_transmission_scheduled: AtomicU64::new(0),
                endpoint_udp_transmission_rejected: AtomicU64::new(0),
                endpoint_udp_packet_transmitted: AtomicU64::new(0),
                endpoint_udp_transmit_errored: AtomicU64::new(0),
                endpoint_udp_packet_received: AtomicU64::new(0),
                endpoint_udp_receive_errored: AtomicU64::new(0),
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
                path_secret_map_id_write_lock: AtomicU64::new(0),
                path_secret_map_address_write_lock: AtomicU64::new(0),
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
        fn on_dc_connection_timeout(
            &self,
            meta: &api::EndpointMeta,
            event: &api::DcConnectionTimeout,
        ) {
            self.dc_connection_timeout.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpImmediateTransmissionScheduled,
        ) {
            self.endpoint_udp_immediate_transmission_scheduled
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionScheduled,
        ) {
            self.endpoint_udp_transmission_scheduled
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_transmission_rejected(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmissionRejected,
        ) {
            self.endpoint_udp_transmission_rejected
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_packet_transmitted(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketTransmitted,
        ) {
            self.endpoint_udp_packet_transmitted
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_transmit_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpTransmitErrored,
        ) {
            self.endpoint_udp_transmit_errored
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_packet_received(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpPacketReceived,
        ) {
            self.endpoint_udp_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_receive_errored(
            &self,
            meta: &api::EndpointMeta,
            event: &api::EndpointUdpReceiveErrored,
        ) {
            self.endpoint_udp_receive_errored
                .fetch_add(1, Ordering::Relaxed);
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
        fn on_path_secret_map_id_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapIdWriteLock,
        ) {
            self.path_secret_map_id_write_lock
                .fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            let out = format!("{meta:?} {event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_write_lock(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapAddressWriteLock,
        ) {
            self.path_secret_map_address_write_lock
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
        pub stream_write_key_updated: AtomicU64,
        pub stream_read_key_updated: AtomicU64,
        pub endpoint_initialized: AtomicU64,
        pub dc_connection_timeout: AtomicU64,
        pub endpoint_udp_immediate_transmission_scheduled: AtomicU64,
        pub endpoint_udp_transmission_scheduled: AtomicU64,
        pub endpoint_udp_transmission_rejected: AtomicU64,
        pub endpoint_udp_packet_transmitted: AtomicU64,
        pub endpoint_udp_transmit_errored: AtomicU64,
        pub endpoint_udp_packet_received: AtomicU64,
        pub endpoint_udp_receive_errored: AtomicU64,
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
        pub path_secret_map_id_write_lock: AtomicU64,
        pub path_secret_map_address_write_lock: AtomicU64,
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
                stream_write_key_updated: AtomicU64::new(0),
                stream_read_key_updated: AtomicU64::new(0),
                endpoint_initialized: AtomicU64::new(0),
                dc_connection_timeout: AtomicU64::new(0),
                endpoint_udp_immediate_transmission_scheduled: AtomicU64::new(0),
                endpoint_udp_transmission_scheduled: AtomicU64::new(0),
                endpoint_udp_transmission_rejected: AtomicU64::new(0),
                endpoint_udp_packet_transmitted: AtomicU64::new(0),
                endpoint_udp_transmit_errored: AtomicU64::new(0),
                endpoint_udp_packet_received: AtomicU64::new(0),
                endpoint_udp_receive_errored: AtomicU64::new(0),
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
                path_secret_map_id_write_lock: AtomicU64::new(0),
                path_secret_map_address_write_lock: AtomicU64::new(0),
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_dc_connection_timeout(&self, event: builder::DcConnectionTimeout) {
            self.dc_connection_timeout.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_immediate_transmission_scheduled(
            &self,
            event: builder::EndpointUdpImmediateTransmissionScheduled,
        ) {
            self.endpoint_udp_immediate_transmission_scheduled
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_transmission_scheduled(
            &self,
            event: builder::EndpointUdpTransmissionScheduled,
        ) {
            self.endpoint_udp_transmission_scheduled
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_transmission_rejected(
            &self,
            event: builder::EndpointUdpTransmissionRejected,
        ) {
            self.endpoint_udp_transmission_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_packet_transmitted(&self, event: builder::EndpointUdpPacketTransmitted) {
            self.endpoint_udp_packet_transmitted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_transmit_errored(&self, event: builder::EndpointUdpTransmitErrored) {
            self.endpoint_udp_transmit_errored
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_packet_received(&self, event: builder::EndpointUdpPacketReceived) {
            self.endpoint_udp_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_endpoint_udp_receive_errored(&self, event: builder::EndpointUdpReceiveErrored) {
            self.endpoint_udp_receive_errored
                .fetch_add(1, Ordering::Relaxed);
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
        fn on_path_secret_map_id_write_lock(&self, event: builder::PathSecretMapIdWriteLock) {
            self.path_secret_map_id_write_lock
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            let out = format!("{event:?}");
            self.output.lock().unwrap().push(out);
        }
        fn on_path_secret_map_address_write_lock(
            &self,
            event: builder::PathSecretMapAddressWriteLock,
        ) {
            self.path_secret_map_address_write_lock
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
        fn on_stream_read_key_updated(&self, event: builder::StreamReadKeyUpdated) {
            self.stream_read_key_updated.fetch_add(1, Ordering::Relaxed);
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

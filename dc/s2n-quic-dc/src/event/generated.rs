// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use super::*;
pub mod api {
    #![doc = r" This module contains events that are emitted to the [`Subscriber`](crate::event::Subscriber)"]
    use super::*;
    pub use s2n_quic_core::event::api::{EndpointType, SocketAddress, Subject};
    pub use traits::Subscriber;
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ConnectionMeta {
        pub id: u64,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ConnectionMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ConnectionMeta");
            fmt.field("id", &self.id);
            fmt.finish()
        }
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct EndpointMeta {}
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for EndpointMeta {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("EndpointMeta");
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
    pub struct ApplicationWrite {
        #[doc = " The number of bytes that the application tried to write"]
        pub total_len: usize,
        #[doc = " The amount that was written"]
        pub write_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ApplicationWrite {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ApplicationWrite");
            fmt.field("total_len", &self.total_len);
            fmt.field("write_len", &self.write_len);
            fmt.finish()
        }
    }
    impl Event for ApplicationWrite {
        const NAME: &'static str = "application:write";
    }
    #[derive(Clone, Debug)]
    #[non_exhaustive]
    pub struct ApplicationRead {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount that was read"]
        pub read_len: usize,
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for ApplicationRead {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("ApplicationRead");
            fmt.field("capacity", &self.capacity);
            fmt.field("read_len", &self.read_len);
            fmt.finish()
        }
    }
    impl Event for ApplicationRead {
        const NAME: &'static str = "application:read";
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
    }
    #[cfg(any(test, feature = "testing"))]
    impl crate::event::snapshot::Fmt for PathSecretMapUninitialized {
        fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::fmt::Result {
            let mut fmt = fmt.debug_struct("PathSecretMapUninitialized");
            fmt.field("capacity", &self.capacity);
            fmt.field("entries", &self.entries);
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
        const NAME: &'static str = "path_secret_map:entry_replaced";
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
        const NAME: &'static str = "path_secret_map:entry_replaced";
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
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            let id = context.id();
            let api::ApplicationWrite {
                total_len,
                write_len,
            } = event;
            tracing :: event ! (target : "application_write" , parent : id , tracing :: Level :: DEBUG , total_len = tracing :: field :: debug (total_len) , write_len = tracing :: field :: debug (write_len));
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            _meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            let id = context.id();
            let api::ApplicationRead { capacity, read_len } = event;
            tracing :: event ! (target : "application_read" , parent : id , tracing :: Level :: DEBUG , capacity = tracing :: field :: debug (capacity) , read_len = tracing :: field :: debug (read_len));
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
            tracing :: event ! (target : "endpoint_initialized" , parent : parent , tracing :: Level :: DEBUG , acceptor_addr = tracing :: field :: debug (acceptor_addr) , handshake_addr = tracing :: field :: debug (handshake_addr) , tcp = tracing :: field :: debug (tcp) , udp = tracing :: field :: debug (udp));
        }
        #[inline]
        fn on_path_secret_map_initialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapInitialized,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapInitialized { capacity } = event;
            tracing :: event ! (target : "path_secret_map_initialized" , parent : parent , tracing :: Level :: DEBUG , capacity = tracing :: field :: debug (capacity));
        }
        #[inline]
        fn on_path_secret_map_uninitialized(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapUninitialized,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapUninitialized { capacity, entries } = event;
            tracing :: event ! (target : "path_secret_map_uninitialized" , parent : parent , tracing :: Level :: DEBUG , capacity = tracing :: field :: debug (capacity) , entries = tracing :: field :: debug (entries));
        }
        #[inline]
        fn on_path_secret_map_background_handshake_requested(
            &self,
            meta: &api::EndpointMeta,
            event: &api::PathSecretMapBackgroundHandshakeRequested,
        ) {
            let parent = self.parent(meta);
            let api::PathSecretMapBackgroundHandshakeRequested { peer_address } = event;
            tracing :: event ! (target : "path_secret_map_background_handshake_requested" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address));
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
            tracing :: event ! (target : "path_secret_map_entry_inserted" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "path_secret_map_entry_ready" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "path_secret_map_entry_replaced" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , new_credential_id = tracing :: field :: debug (new_credential_id) , previous_credential_id = tracing :: field :: debug (previous_credential_id));
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
            tracing :: event ! (target : "unknown_path_secret_packet_sent" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "unknown_path_secret_packet_received" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "unknown_path_secret_packet_accepted" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "unknown_path_secret_packet_rejected" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "unknown_path_secret_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "replay_definitely_detected" , parent : parent , tracing :: Level :: DEBUG , credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id));
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
            tracing :: event ! (target : "replay_potentially_detected" , parent : parent , tracing :: Level :: DEBUG , credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id) , gap = tracing :: field :: debug (gap));
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
            tracing :: event ! (target : "replay_detected_packet_sent" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "replay_detected_packet_received" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "replay_detected_packet_accepted" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id) , key_id = tracing :: field :: debug (key_id));
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
            tracing :: event ! (target : "replay_detected_packet_rejected" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "replay_detected_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "stale_key_packet_sent" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "stale_key_packet_received" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "stale_key_packet_accepted" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "stale_key_packet_rejected" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
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
            tracing :: event ! (target : "stale_key_packet_dropped" , parent : parent , tracing :: Level :: DEBUG , peer_address = tracing :: field :: debug (peer_address) , credential_id = tracing :: field :: debug (credential_id));
        }
    }
}
pub mod builder {
    use super::*;
    pub use s2n_quic_core::event::builder::{EndpointType, SocketAddress, Subject};
    #[derive(Clone, Debug)]
    pub struct ConnectionMeta {
        pub id: u64,
    }
    impl IntoEvent<api::ConnectionMeta> for ConnectionMeta {
        #[inline]
        fn into_event(self) -> api::ConnectionMeta {
            let ConnectionMeta { id } = self;
            api::ConnectionMeta {
                id: id.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct EndpointMeta {}
    impl IntoEvent<api::EndpointMeta> for EndpointMeta {
        #[inline]
        fn into_event(self) -> api::EndpointMeta {
            let EndpointMeta {} = self;
            api::EndpointMeta {}
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
    pub struct ApplicationWrite {
        #[doc = " The number of bytes that the application tried to write"]
        pub total_len: usize,
        #[doc = " The amount that was written"]
        pub write_len: usize,
    }
    impl IntoEvent<api::ApplicationWrite> for ApplicationWrite {
        #[inline]
        fn into_event(self) -> api::ApplicationWrite {
            let ApplicationWrite {
                total_len,
                write_len,
            } = self;
            api::ApplicationWrite {
                total_len: total_len.into_event(),
                write_len: write_len.into_event(),
            }
        }
    }
    #[derive(Clone, Debug)]
    pub struct ApplicationRead {
        #[doc = " The number of bytes that the application tried to read"]
        pub capacity: usize,
        #[doc = " The amount that was read"]
        pub read_len: usize,
    }
    impl IntoEvent<api::ApplicationRead> for ApplicationRead {
        #[inline]
        fn into_event(self) -> api::ApplicationRead {
            let ApplicationRead { capacity, read_len } = self;
            api::ApplicationRead {
                capacity: capacity.into_event(),
                read_len: read_len.into_event(),
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
    }
    impl IntoEvent<api::PathSecretMapUninitialized> for PathSecretMapUninitialized {
        #[inline]
        fn into_event(self) -> api::PathSecretMapUninitialized {
            let PathSecretMapUninitialized { capacity, entries } = self;
            api::PathSecretMapUninitialized {
                capacity: capacity.into_event(),
                entries: entries.into_event(),
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
        type ConnectionContext: 'static + Send;
        #[doc = r" Creates a context to be passed to each connection-related event"]
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext;
        #[doc = "Called when the `ApplicationWrite` event is triggered"]
        #[inline]
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            let _ = context;
            let _ = meta;
            let _ = event;
        }
        #[doc = "Called when the `ApplicationRead` event is triggered"]
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
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
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            self.as_ref().on_application_write(context, meta, event);
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            self.as_ref().on_application_read(context, meta, event);
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
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            (self.0).on_application_write(&context.0, meta, event);
            (self.1).on_application_write(&context.1, meta, event);
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            (self.0).on_application_read(&context.0, meta, event);
            (self.1).on_application_read(&context.1, meta, event);
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
        fn quic_version(&self) -> Option<u32> {
            self.quic_version
        }
    }
    pub trait ConnectionPublisher {
        #[doc = "Publishes a `ApplicationWrite` event to the publisher's subscriber"]
        fn on_application_write(&self, event: builder::ApplicationWrite);
        #[doc = "Publishes a `ApplicationRead` event to the publisher's subscriber"]
        fn on_application_read(&self, event: builder::ApplicationRead);
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
        fn on_application_write(&self, event: builder::ApplicationWrite) {
            let event = event.into_event();
            self.subscriber
                .on_application_write(self.context, &self.meta, &event);
            self.subscriber
                .on_connection_event(self.context, &self.meta, &event);
            self.subscriber.on_event(&self.meta, &event);
        }
        #[inline]
        fn on_application_read(&self, event: builder::ApplicationRead) {
            let event = event.into_event();
            self.subscriber
                .on_application_read(self.context, &self.meta, &event);
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
pub mod metrics {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};
    use s2n_quic_core::event::metrics::Recorder;
    #[derive(Debug)]
    pub struct Subscriber<S: super::Subscriber>
    where
        S::ConnectionContext: Recorder,
    {
        subscriber: S,
    }
    impl<S: super::Subscriber> Subscriber<S>
    where
        S::ConnectionContext: Recorder,
    {
        pub fn new(subscriber: S) -> Self {
            Self { subscriber }
        }
    }
    pub struct Context<R: Recorder> {
        recorder: R,
        application_write: AtomicU32,
        application_read: AtomicU32,
    }
    impl<S: super::Subscriber> super::Subscriber for Subscriber<S>
    where
        S::ConnectionContext: Recorder,
    {
        type ConnectionContext = Context<S::ConnectionContext>;
        fn create_connection_context(
            &self,
            meta: &api::ConnectionMeta,
            info: &api::ConnectionInfo,
        ) -> Self::ConnectionContext {
            Context {
                recorder: self.subscriber.create_connection_context(meta, info),
                application_write: AtomicU32::new(0),
                application_read: AtomicU32::new(0),
            }
        }
        #[inline]
        fn on_application_write(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            context.application_write.fetch_add(1, Ordering::Relaxed);
            self.subscriber
                .on_application_write(&context.recorder, meta, event);
        }
        #[inline]
        fn on_application_read(
            &self,
            context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            context.application_read.fetch_add(1, Ordering::Relaxed);
            self.subscriber
                .on_application_read(&context.recorder, meta, event);
        }
    }
    impl<R: Recorder> Drop for Context<R> {
        fn drop(&mut self) {
            self.recorder.increment_counter(
                "application_write",
                self.application_write.load(Ordering::Relaxed) as _,
            );
            self.recorder.increment_counter(
                "application_read",
                self.application_read.load(Ordering::Relaxed) as _,
            );
        }
    }
}
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::event::snapshot::Location;
    use core::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    pub mod endpoint {
        use super::*;
        pub struct Subscriber {
            location: Option<Location>,
            output: Mutex<Vec<String>>,
            pub endpoint_initialized: AtomicU32,
            pub path_secret_map_initialized: AtomicU32,
            pub path_secret_map_uninitialized: AtomicU32,
            pub path_secret_map_background_handshake_requested: AtomicU32,
            pub path_secret_map_entry_inserted: AtomicU32,
            pub path_secret_map_entry_ready: AtomicU32,
            pub path_secret_map_entry_replaced: AtomicU32,
            pub unknown_path_secret_packet_sent: AtomicU32,
            pub unknown_path_secret_packet_received: AtomicU32,
            pub unknown_path_secret_packet_accepted: AtomicU32,
            pub unknown_path_secret_packet_rejected: AtomicU32,
            pub unknown_path_secret_packet_dropped: AtomicU32,
            pub replay_definitely_detected: AtomicU32,
            pub replay_potentially_detected: AtomicU32,
            pub replay_detected_packet_sent: AtomicU32,
            pub replay_detected_packet_received: AtomicU32,
            pub replay_detected_packet_accepted: AtomicU32,
            pub replay_detected_packet_rejected: AtomicU32,
            pub replay_detected_packet_dropped: AtomicU32,
            pub stale_key_packet_sent: AtomicU32,
            pub stale_key_packet_received: AtomicU32,
            pub stale_key_packet_accepted: AtomicU32,
            pub stale_key_packet_rejected: AtomicU32,
            pub stale_key_packet_dropped: AtomicU32,
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
                    endpoint_initialized: AtomicU32::new(0),
                    path_secret_map_initialized: AtomicU32::new(0),
                    path_secret_map_uninitialized: AtomicU32::new(0),
                    path_secret_map_background_handshake_requested: AtomicU32::new(0),
                    path_secret_map_entry_inserted: AtomicU32::new(0),
                    path_secret_map_entry_ready: AtomicU32::new(0),
                    path_secret_map_entry_replaced: AtomicU32::new(0),
                    unknown_path_secret_packet_sent: AtomicU32::new(0),
                    unknown_path_secret_packet_received: AtomicU32::new(0),
                    unknown_path_secret_packet_accepted: AtomicU32::new(0),
                    unknown_path_secret_packet_rejected: AtomicU32::new(0),
                    unknown_path_secret_packet_dropped: AtomicU32::new(0),
                    replay_definitely_detected: AtomicU32::new(0),
                    replay_potentially_detected: AtomicU32::new(0),
                    replay_detected_packet_sent: AtomicU32::new(0),
                    replay_detected_packet_received: AtomicU32::new(0),
                    replay_detected_packet_accepted: AtomicU32::new(0),
                    replay_detected_packet_rejected: AtomicU32::new(0),
                    replay_detected_packet_dropped: AtomicU32::new(0),
                    stale_key_packet_sent: AtomicU32::new(0),
                    stale_key_packet_received: AtomicU32::new(0),
                    stale_key_packet_accepted: AtomicU32::new(0),
                    stale_key_packet_rejected: AtomicU32::new(0),
                    stale_key_packet_dropped: AtomicU32::new(0),
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
            fn on_stale_key_packet_sent(
                &self,
                meta: &api::EndpointMeta,
                event: &api::StaleKeyPacketSent,
            ) {
                self.stale_key_packet_sent.fetch_add(1, Ordering::Relaxed);
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
        }
    }
    #[derive(Debug)]
    pub struct Subscriber {
        location: Option<Location>,
        output: Mutex<Vec<String>>,
        pub application_write: AtomicU32,
        pub application_read: AtomicU32,
        pub endpoint_initialized: AtomicU32,
        pub path_secret_map_initialized: AtomicU32,
        pub path_secret_map_uninitialized: AtomicU32,
        pub path_secret_map_background_handshake_requested: AtomicU32,
        pub path_secret_map_entry_inserted: AtomicU32,
        pub path_secret_map_entry_ready: AtomicU32,
        pub path_secret_map_entry_replaced: AtomicU32,
        pub unknown_path_secret_packet_sent: AtomicU32,
        pub unknown_path_secret_packet_received: AtomicU32,
        pub unknown_path_secret_packet_accepted: AtomicU32,
        pub unknown_path_secret_packet_rejected: AtomicU32,
        pub unknown_path_secret_packet_dropped: AtomicU32,
        pub replay_definitely_detected: AtomicU32,
        pub replay_potentially_detected: AtomicU32,
        pub replay_detected_packet_sent: AtomicU32,
        pub replay_detected_packet_received: AtomicU32,
        pub replay_detected_packet_accepted: AtomicU32,
        pub replay_detected_packet_rejected: AtomicU32,
        pub replay_detected_packet_dropped: AtomicU32,
        pub stale_key_packet_sent: AtomicU32,
        pub stale_key_packet_received: AtomicU32,
        pub stale_key_packet_accepted: AtomicU32,
        pub stale_key_packet_rejected: AtomicU32,
        pub stale_key_packet_dropped: AtomicU32,
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
                application_write: AtomicU32::new(0),
                application_read: AtomicU32::new(0),
                endpoint_initialized: AtomicU32::new(0),
                path_secret_map_initialized: AtomicU32::new(0),
                path_secret_map_uninitialized: AtomicU32::new(0),
                path_secret_map_background_handshake_requested: AtomicU32::new(0),
                path_secret_map_entry_inserted: AtomicU32::new(0),
                path_secret_map_entry_ready: AtomicU32::new(0),
                path_secret_map_entry_replaced: AtomicU32::new(0),
                unknown_path_secret_packet_sent: AtomicU32::new(0),
                unknown_path_secret_packet_received: AtomicU32::new(0),
                unknown_path_secret_packet_accepted: AtomicU32::new(0),
                unknown_path_secret_packet_rejected: AtomicU32::new(0),
                unknown_path_secret_packet_dropped: AtomicU32::new(0),
                replay_definitely_detected: AtomicU32::new(0),
                replay_potentially_detected: AtomicU32::new(0),
                replay_detected_packet_sent: AtomicU32::new(0),
                replay_detected_packet_received: AtomicU32::new(0),
                replay_detected_packet_accepted: AtomicU32::new(0),
                replay_detected_packet_rejected: AtomicU32::new(0),
                replay_detected_packet_dropped: AtomicU32::new(0),
                stale_key_packet_sent: AtomicU32::new(0),
                stale_key_packet_received: AtomicU32::new(0),
                stale_key_packet_accepted: AtomicU32::new(0),
                stale_key_packet_rejected: AtomicU32::new(0),
                stale_key_packet_dropped: AtomicU32::new(0),
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
        fn on_application_write(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationWrite,
        ) {
            self.application_write.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
            }
        }
        fn on_application_read(
            &self,
            _context: &Self::ConnectionContext,
            meta: &api::ConnectionMeta,
            event: &api::ApplicationRead,
        ) {
            self.application_read.fetch_add(1, Ordering::Relaxed);
            if self.location.is_some() {
                let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
                let event = crate::event::snapshot::Fmt::to_snapshot(event);
                self.output
                    .lock()
                    .unwrap()
                    .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
        }
        fn on_stale_key_packet_sent(
            &self,
            meta: &api::EndpointMeta,
            event: &api::StaleKeyPacketSent,
        ) {
            self.stale_key_packet_sent.fetch_add(1, Ordering::Relaxed);
            let meta = crate::event::snapshot::Fmt::to_snapshot(meta);
            let event = crate::event::snapshot::Fmt::to_snapshot(event);
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
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
            self.output
                .lock()
                .unwrap()
                .push(format!("{meta:?} {event:?}"));
        }
    }
    #[derive(Debug)]
    pub struct Publisher {
        location: Option<Location>,
        output: Mutex<Vec<String>>,
        pub application_write: AtomicU32,
        pub application_read: AtomicU32,
        pub endpoint_initialized: AtomicU32,
        pub path_secret_map_initialized: AtomicU32,
        pub path_secret_map_uninitialized: AtomicU32,
        pub path_secret_map_background_handshake_requested: AtomicU32,
        pub path_secret_map_entry_inserted: AtomicU32,
        pub path_secret_map_entry_ready: AtomicU32,
        pub path_secret_map_entry_replaced: AtomicU32,
        pub unknown_path_secret_packet_sent: AtomicU32,
        pub unknown_path_secret_packet_received: AtomicU32,
        pub unknown_path_secret_packet_accepted: AtomicU32,
        pub unknown_path_secret_packet_rejected: AtomicU32,
        pub unknown_path_secret_packet_dropped: AtomicU32,
        pub replay_definitely_detected: AtomicU32,
        pub replay_potentially_detected: AtomicU32,
        pub replay_detected_packet_sent: AtomicU32,
        pub replay_detected_packet_received: AtomicU32,
        pub replay_detected_packet_accepted: AtomicU32,
        pub replay_detected_packet_rejected: AtomicU32,
        pub replay_detected_packet_dropped: AtomicU32,
        pub stale_key_packet_sent: AtomicU32,
        pub stale_key_packet_received: AtomicU32,
        pub stale_key_packet_accepted: AtomicU32,
        pub stale_key_packet_rejected: AtomicU32,
        pub stale_key_packet_dropped: AtomicU32,
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
                application_write: AtomicU32::new(0),
                application_read: AtomicU32::new(0),
                endpoint_initialized: AtomicU32::new(0),
                path_secret_map_initialized: AtomicU32::new(0),
                path_secret_map_uninitialized: AtomicU32::new(0),
                path_secret_map_background_handshake_requested: AtomicU32::new(0),
                path_secret_map_entry_inserted: AtomicU32::new(0),
                path_secret_map_entry_ready: AtomicU32::new(0),
                path_secret_map_entry_replaced: AtomicU32::new(0),
                unknown_path_secret_packet_sent: AtomicU32::new(0),
                unknown_path_secret_packet_received: AtomicU32::new(0),
                unknown_path_secret_packet_accepted: AtomicU32::new(0),
                unknown_path_secret_packet_rejected: AtomicU32::new(0),
                unknown_path_secret_packet_dropped: AtomicU32::new(0),
                replay_definitely_detected: AtomicU32::new(0),
                replay_potentially_detected: AtomicU32::new(0),
                replay_detected_packet_sent: AtomicU32::new(0),
                replay_detected_packet_received: AtomicU32::new(0),
                replay_detected_packet_accepted: AtomicU32::new(0),
                replay_detected_packet_rejected: AtomicU32::new(0),
                replay_detected_packet_dropped: AtomicU32::new(0),
                stale_key_packet_sent: AtomicU32::new(0),
                stale_key_packet_received: AtomicU32::new(0),
                stale_key_packet_accepted: AtomicU32::new(0),
                stale_key_packet_rejected: AtomicU32::new(0),
                stale_key_packet_dropped: AtomicU32::new(0),
            }
        }
    }
    impl super::EndpointPublisher for Publisher {
        fn on_endpoint_initialized(&self, event: builder::EndpointInitialized) {
            self.endpoint_initialized.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_path_secret_map_initialized(&self, event: builder::PathSecretMapInitialized) {
            self.path_secret_map_initialized
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_path_secret_map_uninitialized(&self, event: builder::PathSecretMapUninitialized) {
            self.path_secret_map_uninitialized
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_path_secret_map_background_handshake_requested(
            &self,
            event: builder::PathSecretMapBackgroundHandshakeRequested,
        ) {
            self.path_secret_map_background_handshake_requested
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_path_secret_map_entry_inserted(&self, event: builder::PathSecretMapEntryInserted) {
            self.path_secret_map_entry_inserted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_path_secret_map_entry_ready(&self, event: builder::PathSecretMapEntryReady) {
            self.path_secret_map_entry_ready
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_path_secret_map_entry_replaced(&self, event: builder::PathSecretMapEntryReplaced) {
            self.path_secret_map_entry_replaced
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_unknown_path_secret_packet_sent(&self, event: builder::UnknownPathSecretPacketSent) {
            self.unknown_path_secret_packet_sent
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_unknown_path_secret_packet_received(
            &self,
            event: builder::UnknownPathSecretPacketReceived,
        ) {
            self.unknown_path_secret_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_unknown_path_secret_packet_accepted(
            &self,
            event: builder::UnknownPathSecretPacketAccepted,
        ) {
            self.unknown_path_secret_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_unknown_path_secret_packet_rejected(
            &self,
            event: builder::UnknownPathSecretPacketRejected,
        ) {
            self.unknown_path_secret_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_unknown_path_secret_packet_dropped(
            &self,
            event: builder::UnknownPathSecretPacketDropped,
        ) {
            self.unknown_path_secret_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_definitely_detected(&self, event: builder::ReplayDefinitelyDetected) {
            self.replay_definitely_detected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_potentially_detected(&self, event: builder::ReplayPotentiallyDetected) {
            self.replay_potentially_detected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_detected_packet_sent(&self, event: builder::ReplayDetectedPacketSent) {
            self.replay_detected_packet_sent
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_detected_packet_received(&self, event: builder::ReplayDetectedPacketReceived) {
            self.replay_detected_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_detected_packet_accepted(&self, event: builder::ReplayDetectedPacketAccepted) {
            self.replay_detected_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_detected_packet_rejected(&self, event: builder::ReplayDetectedPacketRejected) {
            self.replay_detected_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_replay_detected_packet_dropped(&self, event: builder::ReplayDetectedPacketDropped) {
            self.replay_detected_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_stale_key_packet_sent(&self, event: builder::StaleKeyPacketSent) {
            self.stale_key_packet_sent.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_stale_key_packet_received(&self, event: builder::StaleKeyPacketReceived) {
            self.stale_key_packet_received
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_stale_key_packet_accepted(&self, event: builder::StaleKeyPacketAccepted) {
            self.stale_key_packet_accepted
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_stale_key_packet_rejected(&self, event: builder::StaleKeyPacketRejected) {
            self.stale_key_packet_rejected
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn on_stale_key_packet_dropped(&self, event: builder::StaleKeyPacketDropped) {
            self.stale_key_packet_dropped
                .fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            let event = crate::event::snapshot::Fmt::to_snapshot(&event);
            self.output.lock().unwrap().push(format!("{event:?}"));
        }
        fn quic_version(&self) -> Option<u32> {
            Some(1)
        }
    }
    impl super::ConnectionPublisher for Publisher {
        fn on_application_write(&self, event: builder::ApplicationWrite) {
            self.application_write.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                self.output.lock().unwrap().push(format!("{event:?}"));
            }
        }
        fn on_application_read(&self, event: builder::ApplicationRead) {
            self.application_read.fetch_add(1, Ordering::Relaxed);
            let event = event.into_event();
            if self.location.is_some() {
                let event = crate::event::snapshot::Fmt::to_snapshot(&event);
                self.output.lock().unwrap().push(format!("{event:?}"));
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

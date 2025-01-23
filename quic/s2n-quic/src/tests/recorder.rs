// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

macro_rules! event_recorder {
    ($sub:ident, $event:ident, $method:ident) => {
        event_recorder!($sub, $event, $method, events::$event, {
            |event: &events::$event, storage: &mut Vec<events::$event>| storage.push(event.clone())
        });
    };
    ($sub:ident, $event:ident, $method:ident, $storage:ty, $store:expr) => {
        #[derive(Clone, Default)]
        pub struct $sub {
            pub events: Arc<Mutex<Vec<$storage>>>,
        }

        #[allow(dead_code)]
        impl $sub {
            pub fn new() -> Self {
                Self::default()
            }

            pub fn events(&self) -> Arc<Mutex<Vec<$storage>>> {
                self.events.clone()
            }

            pub fn any<F: FnMut(&$storage) -> bool>(&self, f: F) -> bool {
                let events = self.events.lock().unwrap();
                events.iter().any(f)
            }
        }

        impl events::Subscriber for $sub {
            type ConnectionContext = $sub;

            fn create_connection_context(
                &mut self,
                _meta: &events::ConnectionMeta,
                _info: &events::ConnectionInfo,
            ) -> Self::ConnectionContext {
                self.clone()
            }

            fn $method(
                &mut self,
                context: &mut Self::ConnectionContext,
                _meta: &events::ConnectionMeta,
                event: &events::$event,
            ) {
                let store = $store;
                let mut buffer = context.events.lock().unwrap();
                store(event, &mut buffer);
            }
        }
    };
}

event_recorder!(FrameSent, FrameSent, on_frame_sent);
event_recorder!(PacketSent, PacketSent, on_packet_sent);
event_recorder!(MtuUpdated, MtuUpdated, on_mtu_updated);
event_recorder!(
    PathUpdated,
    RecoveryMetrics,
    on_recovery_metrics,
    SocketAddr,
    |event: &events::RecoveryMetrics, storage: &mut Vec<SocketAddr>| {
        let addr: SocketAddr = event.path.local_addr.to_string().parse().unwrap();
        if storage.last().map_or(true, |prev| *prev != addr) {
            storage.push(addr);
        }
    }
);
event_recorder!(
    Pto,
    RecoveryMetrics,
    on_recovery_metrics,
    u32,
    |event: &events::RecoveryMetrics, storage: &mut Vec<u32>| {
        storage.push(event.pto_count);
    }
);
event_recorder!(
    HandshakeStatus,
    HandshakeStatusUpdated,
    on_handshake_status_updated
);

event_recorder!(
    ActivePathUpdated,
    ActivePathUpdated,
    on_active_path_updated,
    SocketAddr,
    |event: &events::ActivePathUpdated, storage: &mut Vec<SocketAddr>| {
        let addr = (&event.active.remote_addr).into();
        storage.push(addr);
    }
);

event_recorder!(
    PacketDropped,
    PacketDropped,
    on_packet_dropped,
    PacketDropReason,
    |event: &events::PacketDropped, storage: &mut Vec<PacketDropReason>| {
        if let Ok(reason) = (&event.reason).try_into() {
            storage.push(reason);
        }
    }
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketDropReason {
    ConnectionError,
    HandshakeNotComplete,
    VersionMismatch,
    ConnectionIdMismatch,
    UnprotectFailed,
    DecryptionFailed,
    DecodingFailed,
    NonEmptyRetryToken,
    RetryDiscarded,
    UndersizedInitialPacket,
}

impl<'a> TryFrom<&events::PacketDropReason<'a>> for PacketDropReason {
    type Error = ();

    fn try_from(reason: &events::PacketDropReason<'a>) -> Result<Self, ()> {
        use events::PacketDropReason::*;

        Ok(match reason {
            ConnectionError { .. } => Self::ConnectionError,
            HandshakeNotComplete { .. } => Self::HandshakeNotComplete,
            VersionMismatch { .. } => Self::VersionMismatch,
            ConnectionIdMismatch { .. } => Self::ConnectionIdMismatch,
            UnprotectFailed { .. } => Self::UnprotectFailed,
            DecryptionFailed { .. } => Self::DecryptionFailed,
            DecodingFailed { .. } => Self::DecodingFailed,
            NonEmptyRetryToken { .. } => Self::NonEmptyRetryToken,
            RetryDiscarded { .. } => Self::RetryDiscarded,
            UndersizedInitialPacket { .. } => Self::UndersizedInitialPacket,
            _ => return Err(()),
        })
    }
}

event_recorder!(
    PacketSkipped,
    PacketSkipped,
    on_packet_skipped,
    events::PacketSkipReason,
    |event: &events::PacketSkipped, storage: &mut Vec<events::PacketSkipReason>| {
        storage.push(event.reason.clone());
    }
);

event_recorder!(
    ConnectionStarted,
    ConnectionStarted,
    on_connection_started,
    SocketAddr,
    |event: &events::ConnectionStarted, storage: &mut Vec<SocketAddr>| {
        let addr: SocketAddr = event.path.local_addr.to_string().parse().unwrap();
        storage.push(addr);
    }
);

use s2n_quic_core::event::api::DatagramDropReason;
#[derive(Debug)]
pub struct DatagramDroppedEvent {
    pub remote_addr: SocketAddr,
    pub reason: DatagramDropReason,
}

impl<'a> From<&events::DatagramDropped<'a>> for DatagramDroppedEvent {
    fn from(value: &events::DatagramDropped<'a>) -> Self {
        DatagramDroppedEvent {
            remote_addr: value.remote_addr.to_string().parse().unwrap(),
            reason: value.reason.clone(),
        }
    }
}

event_recorder!(
    DatagramDropped,
    DatagramDropped,
    on_datagram_dropped,
    DatagramDroppedEvent,
    |event: &events::DatagramDropped, storage: &mut Vec<DatagramDroppedEvent>| {
        storage.push(event.into());
    }
);

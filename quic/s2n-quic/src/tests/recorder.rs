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

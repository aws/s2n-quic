// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

enum Subject {
    Endpoint,
    Connection {
        id: u64,
    },
}

#[c_type(s2n_event_connection_meta)]
struct ConnectionMeta {
    id: u64,
    timestamp: Timestamp,
}

#[repr(C)]
#[allow(non_camel_case_types)]
struct s2n_event_connection_meta {
    timestamp: u64,
}

impl IntoEvent<builder::ConnectionMeta> for &c_ffi::s2n_event_connection_meta {
    fn into_event(self) -> builder::ConnectionMeta {
        let duration = Duration::from_nanos(self.timestamp);
        let timestamp = unsafe {
            s2n_quic_core::time::Timestamp::from_duration(duration).into_event()
        };
        builder::ConnectionMeta {
            id: 0,
            timestamp,
        }
    }
}

struct EndpointMeta {}

#[c_type(s2n_event_connection_info)]
struct ConnectionInfo {}

#[repr(C)]
#[allow(non_camel_case_types)]
struct s2n_event_connection_info {}

impl IntoEvent<builder::ConnectionInfo> for &c_ffi::s2n_event_connection_info {
    fn into_event(self) -> builder::ConnectionInfo {
        builder::ConnectionInfo {}
    }
}

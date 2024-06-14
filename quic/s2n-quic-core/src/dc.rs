// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::Limits,
    event::{
        api::{EndpointType, SocketAddress},
        IntoEvent as _,
    },
    inet,
    transport::parameters::{DcSupportedVersions, InitialFlowControlLimits},
    varint::VarInt,
};
use core::time::Duration;

mod disabled;
mod traits;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use disabled::*;
pub use traits::*;

pub type Version = u32;

// dc versions supported by this code, in order of preference (SUPPORTED_VERSIONS[0] is most preferred)
pub const SUPPORTED_VERSIONS: [Version; 1] = [0x0];

/// Called on the server to select the dc version to use (if any)
///
/// The server's version preference takes precedence
pub fn select_version(client_supported_versions: DcSupportedVersions) -> Option<Version> {
    let client_supported_versions = client_supported_versions.into_iter().as_slice();
    SUPPORTED_VERSIONS
        .iter()
        .find(|&supported_version| client_supported_versions.contains(supported_version))
        .copied()
}

/// Information about the connection that may be used
/// when create a new dc path
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ConnectionInfo<'a> {
    /// The address (IP + Port) of the remote peer
    pub remote_address: SocketAddress<'a>,
    /// The dc version that has been negotiated
    pub dc_version: u32,
    /// Various settings relevant to the dc path
    pub application_params: ApplicationParams,
    /// The local endpoint type (client or server)
    pub endpoint_type: EndpointType,
}

impl<'a> ConnectionInfo<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(
        remote_address: &'a inet::SocketAddress,
        dc_version: Version,
        application_params: ApplicationParams,
        endpoint_type: EndpointType,
    ) -> Self {
        Self {
            remote_address: remote_address.into_event(),
            dc_version,
            application_params,
            endpoint_type,
        }
    }
}

/// Various settings relevant to the dc path
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ApplicationParams {
    pub remote_max_data: VarInt,
    pub local_send_max_data: VarInt,
    pub local_recv_max_data: VarInt,
    pub max_idle_timeout: Option<Duration>,
    pub max_ack_delay: Duration,
}

impl ApplicationParams {
    pub fn new(peer_flow_control_limits: &InitialFlowControlLimits, limits: &Limits) -> Self {
        Self {
            remote_max_data: peer_flow_control_limits.max_data,
            local_send_max_data: limits.initial_stream_limits().max_data_bidi_local,
            local_recv_max_data: limits.initial_stream_limits().max_data_bidi_remote,
            max_idle_timeout: limits.max_idle_timeout(),
            max_ack_delay: limits.max_ack_delay.into(),
        }
    }
}

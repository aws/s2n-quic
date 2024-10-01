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
use core::{num::NonZeroU32, time::Duration};

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

/// Information about a received datagram that may be used
/// when parsing it for a secret control packet
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct DatagramInfo<'a> {
    /// The address (IP + Port) of the remote peer
    pub remote_address: SocketAddress<'a>,
}

impl<'a> DatagramInfo<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(remote_address: &'a inet::SocketAddress) -> Self {
        Self {
            remote_address: remote_address.into_event(),
        }
    }
}

/// Various settings relevant to the dc path
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ApplicationParams {
    pub max_datagram_size: u16,
    pub remote_max_data: VarInt,
    pub local_send_max_data: VarInt,
    pub local_recv_max_data: VarInt,
    // milliseconds (stored this way to reduce size)
    pub max_idle_timeout: Option<NonZeroU32>,
    pub max_ack_delay: Duration,
}

impl ApplicationParams {
    pub fn new(
        max_datagram_size: u16,
        peer_flow_control_limits: &InitialFlowControlLimits,
        limits: &Limits,
    ) -> Self {
        Self {
            max_datagram_size,
            remote_max_data: peer_flow_control_limits.max_data,
            local_send_max_data: limits.initial_stream_limits().max_data_bidi_local,
            local_recv_max_data: limits.initial_stream_limits().max_data_bidi_remote,
            max_idle_timeout: limits
                .max_idle_timeout()
                // If > u32::MAX, treat as not having an idle timeout, that's ~50 days.
                .and_then(|v| v.as_millis().try_into().ok())
                .and_then(NonZeroU32::new),
            max_ack_delay: limits.max_ack_delay.into(),
        }
    }

    pub fn max_idle_timeout(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.max_idle_timeout?.get() as u64))
    }
}

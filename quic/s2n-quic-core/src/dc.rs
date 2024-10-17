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
use core::{
    num::NonZeroU32,
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};

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
#[derive(Debug)]
#[non_exhaustive]
pub struct ApplicationParams {
    pub max_datagram_size: AtomicU16,
    pub remote_max_data: VarInt,
    pub local_send_max_data: VarInt,
    pub local_recv_max_data: VarInt,
    // Actually a Duration, stored as milliseconds to shrink this struct
    pub max_idle_timeout: Option<NonZeroU32>,
}

impl Clone for ApplicationParams {
    fn clone(&self) -> Self {
        Self {
            max_datagram_size: AtomicU16::new(self.max_datagram_size.load(Ordering::Relaxed)),
            remote_max_data: self.remote_max_data,
            local_send_max_data: self.local_send_max_data,
            local_recv_max_data: self.local_recv_max_data,
            max_idle_timeout: self.max_idle_timeout,
        }
    }
}

impl ApplicationParams {
    pub fn new(
        max_datagram_size: u16,
        peer_flow_control_limits: &InitialFlowControlLimits,
        limits: &Limits,
    ) -> Self {
        Self {
            max_datagram_size: AtomicU16::new(max_datagram_size),
            remote_max_data: peer_flow_control_limits.max_data,
            local_send_max_data: limits.initial_stream_limits().max_data_bidi_local,
            local_recv_max_data: limits.initial_stream_limits().max_data_bidi_remote,
            max_idle_timeout: limits
                .max_idle_timeout()
                // If > u32::MAX, treat as not having an idle timeout, that's ~50 days.
                .and_then(|v| v.as_millis().try_into().ok())
                .and_then(NonZeroU32::new),
        }
    }

    pub fn max_idle_timeout(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.max_idle_timeout?.get() as u64))
    }

    pub fn max_datagram_size(&self) -> u16 {
        self.max_datagram_size.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        connection::Limits, dc::ApplicationParams, transport::parameters::InitialFlowControlLimits,
        varint::VarInt,
    };
    use std::{sync::atomic::Ordering, time::Duration};

    #[test]
    fn clone() {
        let initial_flow_control_limits = InitialFlowControlLimits {
            max_data: VarInt::from_u32(2222),
            ..Default::default()
        };

        let limits = Limits {
            bidirectional_local_data_window: 1234.try_into().unwrap(),
            bidirectional_remote_data_window: 6789.try_into().unwrap(),
            max_idle_timeout: Duration::from_millis(999).try_into().unwrap(),
            ..Default::default()
        };

        let params = ApplicationParams::new(9000, &initial_flow_control_limits, &limits);

        assert_eq!(9000, params.max_datagram_size.load(Ordering::Relaxed));
        assert_eq!(limits.max_idle_timeout(), params.max_idle_timeout());
        assert_eq!(1234, params.local_send_max_data.as_u64());
        assert_eq!(6789, params.local_recv_max_data.as_u64());
        assert_eq!(2222, params.remote_max_data.as_u64());

        let cloned_params = params.clone();

        assert_eq!(
            params.max_datagram_size.load(Ordering::Relaxed),
            cloned_params.max_datagram_size.load(Ordering::Relaxed)
        );
        assert_eq!(params.max_idle_timeout, cloned_params.max_idle_timeout);
        assert_eq!(
            params.local_send_max_data,
            cloned_params.local_send_max_data
        );
        assert_eq!(
            params.local_recv_max_data,
            cloned_params.local_recv_max_data
        );
        assert_eq!(params.remote_max_data, cloned_params.remote_max_data);
    }
}

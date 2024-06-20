// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use s2n_quic_core::{
    dc,
    path::{Handle, MaxMtu, Tuple},
    varint::VarInt,
};

pub mod secret;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub static DEFAULT_MAX_DATA: once_cell::sync::Lazy<VarInt> = once_cell::sync::Lazy::new(|| {
    std::env::var("DC_QUIC_DEFAULT_MAX_DATA")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1u32 << 25)
        .into()
});

pub static DEFAULT_MTU: once_cell::sync::Lazy<MaxMtu> = once_cell::sync::Lazy::new(|| {
    let default_mtu = if cfg!(target_os = "linux") {
        8940
    } else {
        1450
    };

    std::env::var("DC_QUIC_DEFAULT_MTU")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default_mtu)
        .try_into()
        .unwrap()
});

pub static DEFAULT_IDLE_TIMEOUT: once_cell::sync::Lazy<u32> = once_cell::sync::Lazy::new(|| {
    std::env::var("DC_QUIC_DEFAULT_IDLE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::stream::DEFAULT_IDLE_TIMEOUT.as_secs())
        .try_into()
        .unwrap()
});

pub trait Controller {
    type Handle: Handle;

    fn handle(&self) -> &Self::Handle;
}

impl Controller for Tuple {
    type Handle = Self;

    #[inline]
    fn handle(&self) -> &Self::Handle {
        self
    }
}

// TODO: replace with dc::ApplicationParams
#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub max_mtu: MaxMtu,
    pub remote_max_data: VarInt,
    pub local_send_max_data: VarInt,
    pub local_recv_max_data: VarInt,
    pub idle_timeout_secs: u32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            max_mtu: *DEFAULT_MTU,
            remote_max_data: *DEFAULT_MAX_DATA,
            local_send_max_data: *DEFAULT_MAX_DATA,
            local_recv_max_data: *DEFAULT_MAX_DATA,
            idle_timeout_secs: *DEFAULT_IDLE_TIMEOUT,
        }
    }
}

impl Parameters {
    #[inline]
    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.idle_timeout_secs as _)
    }
}

impl From<dc::ApplicationParams> for Parameters {
    fn from(value: dc::ApplicationParams) -> Self {
        Self {
            max_mtu: value.max_mtu,
            remote_max_data: value.remote_max_data,
            local_send_max_data: value.local_send_max_data,
            local_recv_max_data: value.local_recv_max_data,
            idle_timeout_secs: value
                .max_idle_timeout
                .and_then(|timeout| timeout.as_secs().try_into().ok())
                .unwrap_or(u32::MAX),
        }
    }
}

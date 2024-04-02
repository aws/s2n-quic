// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::mem::size_of;
use s2n_quic_core::{
    inet::{ethernet, ipv4, udp},
    path::{mtu, MtuError},
};
use s2n_quic_xdp::umem::DEFAULT_FRAME_SIZE;
use tokio::runtime::Handle;

/// Calculate how much a packet will need for fixed-size headers
const MIN_FRAME_OVERHEAD: u16 =
    (size_of::<ethernet::Header>() + size_of::<ipv4::Header>() + size_of::<udp::Header>()) as _;

#[derive(Debug)]
#[must_use = "Builders do nothing without calling `build`"]
pub struct Builder<Rx = (), Tx = ()> {
    rx: Rx,
    tx: Tx,
    mtu_config_builder: mtu::Builder,
    handle: Option<Handle>,
}

impl Default for Builder<(), ()> {
    fn default() -> Self {
        Self {
            rx: (),
            tx: (),
            mtu_config_builder: mtu::Config::builder()
                .with_max_mtu(DEFAULT_FRAME_SIZE as u16 - MIN_FRAME_OVERHEAD)
                .unwrap(),
            handle: None,
        }
    }
}

impl<Rx, Tx> Builder<Rx, Tx> {
    /// Sets the tokio runtime handle for the provider
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the UMEM frame size for the provider
    pub fn with_frame_size(mut self, frame_size: u16) -> Result<Self, MtuError> {
        self.mtu_config_builder = self
            .mtu_config_builder
            .with_max_mtu(frame_size.saturating_sub(MIN_FRAME_OVERHEAD))?;
        Ok(self)
    }

    /// Sets the RX implementation for the provider
    pub fn with_rx<NewRx>(self, rx: NewRx) -> Builder<NewRx, Tx>
    where
        NewRx: super::rx::Rx,
    {
        let Self {
            tx,
            handle,
            mtu_config_builder,
            ..
        } = self;
        Builder {
            rx,
            tx,
            handle,
            mtu_config_builder,
        }
    }

    /// Sets the TX implementation for the provider
    pub fn with_tx<NewTx>(self, tx: NewTx) -> Builder<Rx, NewTx>
    where
        NewTx: super::tx::Tx,
    {
        let Self {
            rx,
            handle,
            mtu_config_builder,
            ..
        } = self;
        Builder {
            rx,
            tx,
            handle,
            mtu_config_builder,
        }
    }
}

impl<Rx, Tx> Builder<Rx, Tx>
where
    Rx: 'static + super::rx::Rx + Send,
    Tx: 'static + super::tx::Tx<PathHandle = Rx::PathHandle> + Send,
{
    pub fn build(self) -> super::Provider<Rx, Tx> {
        let Self {
            rx,
            tx,
            handle,
            mtu_config_builder,
        } = self;
        super::Provider {
            rx,
            tx,
            handle,
            mtu_config_builder,
        }
    }
}

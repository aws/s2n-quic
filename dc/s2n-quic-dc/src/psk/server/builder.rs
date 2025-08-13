// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret,
    psk::io::{Result, DEFAULT_IDLE_TIMEOUT, DEFAULT_MAX_DATA, DEFAULT_MTU},
};
use s2n_quic::provider::{event::Subscriber as Sub, tls::Provider as Prov};
use std::{net::SocketAddr, time::Duration};

use super::Provider;

pub struct Builder<
    Event: s2n_quic::provider::event::Subscriber = s2n_quic::provider::event::default::Subscriber,
> {
    #[allow(dead_code)]
    pub(crate) event_subscriber: Event,
    pub(crate) data_window: u64,
    pub(crate) mtu: u16,
    pub(crate) max_idle_timeout: Duration,
}

impl Default for Builder<s2n_quic::provider::event::default::Subscriber> {
    fn default() -> Self {
        Self {
            event_subscriber: Default::default(),
            data_window: DEFAULT_MAX_DATA,
            mtu: DEFAULT_MTU,
            max_idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
}

impl<Event: s2n_quic::provider::event::Subscriber> Builder<Event> {
    /// Sets an event subscriber
    pub fn with_event_subscriber<E: s2n_quic::provider::event::Subscriber>(
        self,
        event_subscriber: E,
    ) -> Builder<E> {
        Builder {
            event_subscriber,
            data_window: self.data_window,
            mtu: self.mtu,
            max_idle_timeout: self.max_idle_timeout,
        }
    }

    /// Sets the data window to use for flow control
    pub fn with_data_window(mut self, data_window: u64) -> Self {
        self.data_window = data_window;
        self
    }

    /// Sets the largest maximum transmission unit (MTU) that will be used for transmission
    pub fn with_mtu(mut self, mtu: u16) -> Self {
        self.mtu = mtu;
        self
    }

    /// Sets the maximum amount of time a stream will wait without response before closing with an error
    ///
    /// The default value is 30s. Applications can set this to [`Duration::MAX`] to effectively disable the
    /// timeout.
    pub fn with_max_idle_timeout(mut self, max_idle_timeout: Duration) -> Self {
        self.max_idle_timeout = max_idle_timeout;
        self
    }

    /// Starts the server listening to the given address.
    pub async fn start<
        TlsProvider: Prov + Clone + Send + Sync + 'static,
        Subscriber: Sub + Default + Clone + Send + Sync + 'static,
    >(
        self,
        bind: SocketAddr,
        tls_materials_provider: TlsProvider,
        subscriber: Subscriber,
        map: secret::Map,
    ) -> Result<Provider> {
        let (rx, guard) = Provider::setup::<TlsProvider, Subscriber, Event>(
            bind,
            map.clone(),
            tls_materials_provider,
            subscriber,
            self,
        );
        let local_addr = rx.await??;
        Ok(Provider::new(map, local_addr, guard))
    }

    /// Starts the server listening to the given address, blocking until the server has been bound to the address.
    pub fn start_blocking<
        TlsProvider: Prov + Clone + Send + Sync + 'static,
        Subscriber: Sub + Default + Clone + Send + Sync + 'static,
    >(
        self,
        bind: SocketAddr,
        tls_materials_provider: TlsProvider,
        subscriber: Subscriber,
        map: secret::Map,
    ) -> Result<Provider> {
        let (rx, guard) = Provider::setup::<TlsProvider, Subscriber, Event>(
            bind,
            map.clone(),
            tls_materials_provider,
            subscriber,
            self,
        );
        let local_addr = rx.blocking_recv()??;
        Ok(Provider::new(map, local_addr, guard))
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret,
    psk::io::{
        Result, DEFAULT_IDLE_TIMEOUT, DEFAULT_MAX_DATA, DEFAULT_MTU, DEFAULT_PTO_JITTER_PERCENTAGE,
    },
};
use s2n_quic::{
    provider::{event::Subscriber as Sub, tls::Provider as Prov},
    server::Name,
};
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
    pub(crate) pto_jitter_percentage: u8,
    pub(crate) success_jitter: Duration,
}

impl Default for Builder<s2n_quic::provider::event::default::Subscriber> {
    fn default() -> Self {
        Self {
            event_subscriber: Default::default(),
            data_window: DEFAULT_MAX_DATA,
            mtu: DEFAULT_MTU,
            max_idle_timeout: DEFAULT_IDLE_TIMEOUT,
            pto_jitter_percentage: DEFAULT_PTO_JITTER_PERCENTAGE,
            success_jitter: Duration::from_secs(60),
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
            pto_jitter_percentage: self.pto_jitter_percentage,
            success_jitter: self.success_jitter,
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
    pub fn with_max_idle_timeout(mut self, timeout: Duration) -> Self {
        self.max_idle_timeout = timeout;
        self
    }

    /// Sets the PTO jitter percentage (default: 33)
    ///
    /// Adds random jitter to Probe Timeout (PTO) calculations to prevent synchronized
    /// timeouts across multiple connections. The jitter is applied as a percentage
    /// of the base PTO period, with values between -X% and +X% where X is the
    /// configured percentage.
    ///
    /// Valid range: 0-50% (default: 33%)
    /// - 0%: No jitter
    /// - 1-50%: Applies random jitter within ±percentage of base PTO
    pub fn with_pto_jitter_percentage(mut self, pto_jitter_percentage: u8) -> Self {
        self.pto_jitter_percentage = pto_jitter_percentage;
        self
    }

    /// Sets the period we wait before attempting new handshakes with the same peer (by IP:port),
    /// after a successfully completed handshake.
    ///
    /// This is the upper bound, the jitter is randomized between 1 second and the upper bound.
    ///
    /// This defaults to 1 minute.
    pub fn with_success_jitter(mut self, period: Duration) -> Self {
        self.success_jitter = period;
        self
    }

    /// Bind the client to the given address.
    ///
    /// Typically the address provided can use an ephemeral port.
    pub fn start<
        TlsProvider: Prov + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
    >(
        self,
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: TlsProvider,
        subscriber: Subscriber,
        server_name: Name,
    ) -> Result<Provider> {
        Provider::new::<TlsProvider, Subscriber, Event>(
            addr,
            map,
            tls_materials_provider,
            subscriber,
            self,
            server_name,
        )
    }
}

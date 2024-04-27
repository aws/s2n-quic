// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection, endpoint};
use core::{ops::RangeInclusive, time::Duration};

mod generated;
pub use generated::*;

/// All event types which can be emitted from this library.
pub trait Event: core::fmt::Debug {
    const NAME: &'static str;
}

pub trait IntoEvent<Target> {
    fn into_event(self) -> Target;
}

macro_rules! ident_into_event {
    ($($name:ty),* $(,)?) => {
        $(
            impl IntoEvent<$name> for $name {
                #[inline]
                fn into_event(self) -> Self {
                    self
                }
            }
        )*
    };
}

macro_rules! borrowed_into_event {
    ($($name:ty),* $(,)?) => {
        $(
            impl<'a> IntoEvent<&'a $name> for &'a $name {
                #[inline]
                fn into_event(self) -> Self {
                    self
                }
            }
        )*
    };
}

ident_into_event!(
    u8,
    i8,
    u16,
    i16,
    u32,
    i32,
    u64,
    i64,
    usize,
    isize,
    f32,
    Duration,
    bool,
    connection::Error,
    endpoint::Location,
    [u32; 4],
);
borrowed_into_event!([u8; 4], [u8; 16], [u8], [u32], [&'a [u8]]);

impl<T: IntoEvent<U>, U> IntoEvent<Option<U>> for Option<T> {
    #[inline]
    fn into_event(self) -> Option<U> {
        self.map(IntoEvent::into_event)
    }
}

impl<'a> IntoEvent<&'a str> for &'a str {
    #[inline]
    fn into_event(self) -> Self {
        self
    }
}

impl<T> IntoEvent<RangeInclusive<T>> for RangeInclusive<T> {
    #[inline]
    fn into_event(self) -> RangeInclusive<T> {
        self
    }
}

#[derive(Clone, Debug, Copy)]
pub struct Timestamp(crate::time::Timestamp);

impl Timestamp {
    /// The duration since the start of the s2n-quic process.
    ///
    /// Record the start `SystemTime` at the start of the program
    /// to derive the absolute time at which an event is emitted.
    ///
    /// ```rust
    /// # use s2n_quic_core::{
    /// #    endpoint,
    /// #    event::{self, IntoEvent},
    /// #    time::{Duration, Timestamp},
    /// # };
    ///
    /// let start_time = std::time::SystemTime::now();
    /// // `meta` is included as part of each event
    /// # let meta: event::api::ConnectionMeta = event::builder::ConnectionMeta {
    /// #     endpoint_type: endpoint::Type::Server,
    /// #     id: 0,
    /// #     timestamp: unsafe { Timestamp::from_duration(Duration::from_secs(1) )},
    /// # }.into_event();
    /// let event_time = start_time + meta.timestamp.duration_since_start();
    /// ```
    pub fn duration_since_start(&self) -> Duration {
        // Safety: the duration is relative to start of program. This function along
        // with it's documentation captures this intent.
        unsafe { self.0.as_duration() }
    }

    /// Returns the `Duration` which elapsed since an earlier `Timestamp`.
    /// If `earlier` is more recent, the method returns a `Duration` of 0.
    #[inline]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_duration_since(earlier.0)
    }
}

impl IntoEvent<Timestamp> for crate::time::Timestamp {
    #[inline]
    fn into_event(self) -> Timestamp {
        Timestamp(self)
    }
}

#[derive(Clone)]
pub struct TlsSession<'a> {
    session: &'a dyn crate::crypto::tls::TlsSession,
}

impl<'a> TlsSession<'a> {
    #[doc(hidden)]
    pub fn new(session: &'a dyn crate::crypto::tls::TlsSession) -> TlsSession<'a> {
        TlsSession { session }
    }

    pub fn tls_exporter(
        &self,
        label: &[u8],
        context: &[u8],
        output: &mut [u8],
    ) -> Result<(), crate::crypto::tls::TlsExportError> {
        self.session.tls_exporter(label, context, output)
    }

    pub fn cipher_suite(&self) -> crate::event::api::CipherSuite {
        self.session.cipher_suite().into_event()
    }
}

impl<'a> crate::event::IntoEvent<TlsSession<'a>> for TlsSession<'a> {
    #[inline]
    fn into_event(self) -> Self {
        self
    }
}

impl core::fmt::Debug for TlsSession<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TlsSession").finish_non_exhaustive()
    }
}

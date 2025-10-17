// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection, endpoint};
#[cfg(feature = "alloc")]
use alloc::vec::Vec;
use core::{fmt, ops::RangeInclusive, time::Duration};

mod generated;
pub mod metrics;
pub use generated::*;

#[cfg(any(test, feature = "testing"))]
#[doc(hidden)]
pub mod snapshot;

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
);
borrowed_into_event!(
    [u8; 4],
    [u8; 16],
    [u8],
    [u32],
    [&'a [u8]],
    (dyn core::error::Error + Send + Sync + 'static)
);

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

impl<'a> IntoEvent<&'a (dyn core::any::Any + Send + 'static)>
    for &'a (dyn core::any::Any + Send + 'static)
{
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

#[derive(Clone, Copy)]
pub struct Timestamp(crate::time::Timestamp);

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

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

impl IntoEvent<Timestamp> for Timestamp {
    #[inline]
    fn into_event(self) -> Timestamp {
        self
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

    // Currently intended only for unstable usage
    #[doc(hidden)]
    #[cfg(feature = "alloc")]
    pub fn peer_cert_chain_der(&self) -> Result<Vec<Vec<u8>>, crate::crypto::tls::ChainError> {
        self.session.peer_cert_chain_der()
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

#[cfg(feature = "std")]
impl<'a> IntoEvent<&'a std::io::Error> for &'a std::io::Error {
    #[inline]
    fn into_event(self) -> &'a std::io::Error {
        self
    }
}

/// Provides metadata related to an event
pub trait Meta: core::fmt::Debug {
    /// Returns whether the local endpoint is a Client or Server
    fn endpoint_type(&self) -> &api::EndpointType;

    /// A context from which the event is being emitted
    ///
    /// An event can occur in the context of an Endpoint or Connection
    fn subject(&self) -> api::Subject;

    /// The time the event occurred
    fn timestamp(&self) -> &Timestamp;
}

impl Meta for api::ConnectionMeta {
    fn endpoint_type(&self) -> &api::EndpointType {
        &self.endpoint_type
    }

    fn subject(&self) -> api::Subject {
        api::Subject::Connection { id: self.id }
    }

    fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }
}

impl Meta for api::EndpointMeta {
    fn endpoint_type(&self) -> &api::EndpointType {
        &self.endpoint_type
    }

    fn subject(&self) -> api::Subject {
        api::Subject::Endpoint {}
    }

    fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }
}

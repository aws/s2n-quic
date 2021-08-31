// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::connection;
use core::time::Duration;

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

ident_into_event!(u8, u16, u32, u64, usize, Duration, bool, connection::Error);
borrowed_into_event!([u8; 4], [u8; 16], [u8], [u32], [&'a [u8]]);

impl<T: IntoEvent<U>, U> IntoEvent<Option<U>> for Option<T> {
    #[inline]
    fn into_event(self) -> Option<U> {
        self.map(IntoEvent::into_event)
    }
}

#[derive(Clone, Debug)]
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
}

impl IntoEvent<Timestamp> for crate::time::Timestamp {
    #[inline]
    fn into_event(self) -> Timestamp {
        Timestamp(self)
    }
}

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
    Duration,
    bool,
    connection::Error,
    endpoint::Location,
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

pub mod query {

    //! This module provides `Query` and `QueryMut` traits, which are used for querying the
    //! [`Subscriber::ConnectionContext`](crate::event::Subscriber::ConnectionContext)
    //! on a Subscriber.

    use core::marker::PhantomData;

    pub trait Query {
        fn execute(&mut self, context: &dyn core::any::Any) -> ControlFlow;
    }

    pub trait QueryMut {
        fn execute_mut(&mut self, context: &mut dyn core::any::Any) -> ControlFlow;
    }

    /// Used to tell a query whether it should exit early or go on as usual.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum ControlFlow {
        Continue,
        Break,
    }

    impl ControlFlow {
        #[inline]
        #[must_use]
        pub fn and_then(self, f: impl FnOnce() -> Self) -> Self {
            match self {
                Self::Continue => f(),
                Self::Break => Self::Break,
            }
        }
    }

    /// A type that implements Query and QueryMut traits and only executes once.
    ///
    /// This will execute and short-circuit on the first match.
    pub struct Once<F, EventContext, Outcome> {
        query: Option<F>,
        result: Option<Outcome>,
        context: PhantomData<EventContext>,
    }

    impl<F, EventContext, Outcome> From<Once<F, EventContext, Outcome>> for Result<Outcome, Error> {
        #[inline]
        fn from(query: Once<F, EventContext, Outcome>) -> Self {
            query.result.ok_or(Error::ContextTypeMismatch)
        }
    }

    impl<F, ConnectionContext, Outcome> Once<F, ConnectionContext, Outcome>
    where
        F: FnOnce(&ConnectionContext) -> Outcome,
        ConnectionContext: 'static,
    {
        #[inline]
        pub fn new(query: F) -> Self {
            Self {
                query: Some(query),
                result: None,
                context: PhantomData,
            }
        }
    }

    impl<F, ConnectionContext, Outcome> Once<F, ConnectionContext, Outcome>
    where
        F: FnOnce(&mut ConnectionContext) -> Outcome,
        ConnectionContext: 'static,
    {
        #[inline]
        pub fn new_mut(query: F) -> Self {
            Self {
                query: Some(query),
                result: None,
                context: PhantomData,
            }
        }
    }

    impl<F, EventContext, Outcome> Query for Once<F, EventContext, Outcome>
    where
        F: FnOnce(&EventContext) -> Outcome,
        EventContext: 'static,
    {
        fn execute(&mut self, context: &dyn core::any::Any) -> ControlFlow {
            match context.downcast_ref::<EventContext>() {
                Some(context) => {
                    let query = self.query.take().expect("can only match once");
                    self.result = Some(query(context));
                    ControlFlow::Break
                }
                None => ControlFlow::Continue,
            }
        }
    }

    impl<F, EventContext, Outcome> QueryMut for Once<F, EventContext, Outcome>
    where
        F: FnOnce(&mut EventContext) -> Outcome,
        EventContext: 'static,
    {
        fn execute_mut(&mut self, context: &mut dyn core::any::Any) -> ControlFlow {
            match context.downcast_mut::<EventContext>() {
                Some(context) => {
                    let query = self.query.take().expect("can only match once");
                    self.result = Some(query(context));
                    ControlFlow::Break
                }
                None => ControlFlow::Continue,
            }
        }
    }

    #[non_exhaustive]
    #[derive(Debug, Clone)]
    /// Reason for the failed query.
    pub enum Error {
        /// The connection lock is poisoned and the connection unusable.
        ConnectionLockPoisoned,

        /// The expected query type failed to match any of the configured Subscriber's
        /// context types.
        ContextTypeMismatch,
    }

    impl core::fmt::Display for Error {
        fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
            write!(f, "{:?}", self)
        }
    }

    #[cfg(feature = "std")]
    impl std::error::Error for Error {}
}

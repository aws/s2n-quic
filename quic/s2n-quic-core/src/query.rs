// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module provides `Query` and `QueryMut` traits, which are used for querying
//! and executing functions on several different providers. This includes
//! [`Subscriber::ConnectionContext`](crate::event::Subscriber::ConnectionContext)
//! on a Subscriber and the [`Sender`](crate::datagram::Sender) and
//! [`Receiver`](crate::datagram::Receiver) types on a datagram [`Endpoint`](crate::datagram::Endpoint).

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
pub struct Once<F, Context, Outcome> {
    query: Option<F>,
    result: Option<Outcome>,
    context: PhantomData<Context>,
}

impl<F, Context, Outcome> From<Once<F, Context, Outcome>> for Result<Outcome, Error> {
    #[inline]
    fn from(query: Once<F, Context, Outcome>) -> Self {
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

impl<F, Context, Outcome> Query for Once<F, Context, Outcome>
where
    F: FnOnce(&Context) -> Outcome,
    Context: 'static,
{
    fn execute(&mut self, context: &dyn core::any::Any) -> ControlFlow {
        match context.downcast_ref::<Context>() {
            Some(context) => {
                let query = self.query.take().expect("can only match once");
                self.result = Some(query(context));
                ControlFlow::Break
            }
            None => ControlFlow::Continue,
        }
    }
}

impl<F, Context, Outcome> QueryMut for Once<F, Context, Outcome>
where
    F: FnOnce(&mut Context) -> Outcome,
    Context: 'static,
{
    fn execute_mut(&mut self, context: &mut dyn core::any::Any) -> ControlFlow {
        match context.downcast_mut::<Context>() {
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

    /// The expected query type failed to match any of the configured types.
    ContextTypeMismatch,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

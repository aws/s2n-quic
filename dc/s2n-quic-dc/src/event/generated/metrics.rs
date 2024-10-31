// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{self, api, metrics::Recorder};
use core::sync::atomic::{AtomicU32, Ordering};
pub(crate) mod aggregate;
pub(crate) mod probe;
#[derive(Debug)]
pub struct Subscriber<S: event::Subscriber>
where
    S::ConnectionContext: Recorder,
{
    subscriber: S,
}
impl<S: event::Subscriber> Subscriber<S>
where
    S::ConnectionContext: Recorder,
{
    pub fn new(subscriber: S) -> Self {
        Self { subscriber }
    }
}
pub struct Context<R: Recorder> {
    recorder: R,
    application_write: AtomicU32,
    application_read: AtomicU32,
}
impl<S: event::Subscriber> event::Subscriber for Subscriber<S>
where
    S::ConnectionContext: Recorder,
{
    type ConnectionContext = Context<S::ConnectionContext>;
    fn create_connection_context(
        &self,
        meta: &api::ConnectionMeta,
        info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        Context {
            recorder: self.subscriber.create_connection_context(meta, info),
            application_write: AtomicU32::new(0),
            application_read: AtomicU32::new(0),
        }
    }
    #[inline]
    fn on_application_write(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationWrite,
    ) {
        context.application_write.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_application_write(&context.recorder, meta, event);
    }
    #[inline]
    fn on_application_read(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ApplicationRead,
    ) {
        context.application_read.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_application_read(&context.recorder, meta, event);
    }
}
impl<R: Recorder> Drop for Context<R> {
    fn drop(&mut self) {
        self.recorder.increment_counter(
            "application_write",
            self.application_write.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "application_read",
            self.application_read.load(Ordering::Relaxed) as _,
        );
    }
}

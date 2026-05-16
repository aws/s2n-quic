// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-events` crate and any required
// changes should be made there.

use crate::event::{self, api, metrics::Recorder};
#[allow(unused_imports)]
use core::sync::atomic::{AtomicU64, Ordering};
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
    stream_write_key_updated: AtomicU64,
    stream_read_key_updated: AtomicU64,
}
impl<R: Recorder> Context<R> {
    pub fn inner(&self) -> &R {
        &self.recorder
    }
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.recorder
    }
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
            stream_write_key_updated: AtomicU64::new(0),
            stream_read_key_updated: AtomicU64::new(0),
        }
    }
    #[inline]
    fn on_stream_write_key_updated(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteKeyUpdated,
    ) {
        context
            .stream_write_key_updated
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_key_updated(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_key_updated(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadKeyUpdated,
    ) {
        context
            .stream_read_key_updated
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_key_updated(&context.recorder, meta, event);
    }
}
impl<R: Recorder> Drop for Context<R> {
    fn drop(&mut self) {
        self.recorder.increment_counter(
            "stream_write_key_updated",
            self.stream_write_key_updated.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_key_updated",
            self.stream_read_key_updated.load(Ordering::Relaxed) as _,
        );
    }
}

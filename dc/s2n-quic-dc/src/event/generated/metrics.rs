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
    stream_write_flushed: AtomicU32,
    stream_write_fin_flushed: AtomicU32,
    stream_write_blocked: AtomicU32,
    stream_write_errored: AtomicU32,
    stream_write_shutdown: AtomicU32,
    stream_write_socket_flushed: AtomicU32,
    stream_write_socket_blocked: AtomicU32,
    stream_write_socket_errored: AtomicU32,
    stream_read_flushed: AtomicU32,
    stream_read_fin_flushed: AtomicU32,
    stream_read_blocked: AtomicU32,
    stream_read_errored: AtomicU32,
    stream_read_shutdown: AtomicU32,
    stream_read_socket_flushed: AtomicU32,
    stream_read_socket_blocked: AtomicU32,
    stream_read_socket_errored: AtomicU32,
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
            stream_write_flushed: AtomicU32::new(0),
            stream_write_fin_flushed: AtomicU32::new(0),
            stream_write_blocked: AtomicU32::new(0),
            stream_write_errored: AtomicU32::new(0),
            stream_write_shutdown: AtomicU32::new(0),
            stream_write_socket_flushed: AtomicU32::new(0),
            stream_write_socket_blocked: AtomicU32::new(0),
            stream_write_socket_errored: AtomicU32::new(0),
            stream_read_flushed: AtomicU32::new(0),
            stream_read_fin_flushed: AtomicU32::new(0),
            stream_read_blocked: AtomicU32::new(0),
            stream_read_errored: AtomicU32::new(0),
            stream_read_shutdown: AtomicU32::new(0),
            stream_read_socket_flushed: AtomicU32::new(0),
            stream_read_socket_blocked: AtomicU32::new(0),
            stream_read_socket_errored: AtomicU32::new(0),
        }
    }
    #[inline]
    fn on_stream_write_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteFlushed,
    ) {
        context.stream_write_flushed.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_flushed(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_fin_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteFinFlushed,
    ) {
        context
            .stream_write_fin_flushed
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_fin_flushed(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteBlocked,
    ) {
        context.stream_write_blocked.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_blocked(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteErrored,
    ) {
        context.stream_write_errored.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_errored(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_shutdown(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteShutdown,
    ) {
        context
            .stream_write_shutdown
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_shutdown(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_socket_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteSocketFlushed,
    ) {
        context
            .stream_write_socket_flushed
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_socket_flushed(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_socket_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteSocketBlocked,
    ) {
        context
            .stream_write_socket_blocked
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_socket_blocked(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_write_socket_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamWriteSocketErrored,
    ) {
        context
            .stream_write_socket_errored
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_write_socket_errored(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadFlushed,
    ) {
        context.stream_read_flushed.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_flushed(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_fin_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadFinFlushed,
    ) {
        context
            .stream_read_fin_flushed
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_fin_flushed(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadBlocked,
    ) {
        context.stream_read_blocked.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_blocked(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadErrored,
    ) {
        context.stream_read_errored.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_errored(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_shutdown(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadShutdown,
    ) {
        context.stream_read_shutdown.fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_shutdown(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_socket_flushed(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadSocketFlushed,
    ) {
        context
            .stream_read_socket_flushed
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_socket_flushed(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_socket_blocked(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadSocketBlocked,
    ) {
        context
            .stream_read_socket_blocked
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_socket_blocked(&context.recorder, meta, event);
    }
    #[inline]
    fn on_stream_read_socket_errored(
        &self,
        context: &Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::StreamReadSocketErrored,
    ) {
        context
            .stream_read_socket_errored
            .fetch_add(1, Ordering::Relaxed);
        self.subscriber
            .on_stream_read_socket_errored(&context.recorder, meta, event);
    }
}
impl<R: Recorder> Drop for Context<R> {
    fn drop(&mut self) {
        self.recorder.increment_counter(
            "stream_write_flushed",
            self.stream_write_flushed.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_fin_flushed",
            self.stream_write_fin_flushed.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_blocked",
            self.stream_write_blocked.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_errored",
            self.stream_write_errored.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_shutdown",
            self.stream_write_shutdown.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_socket_flushed",
            self.stream_write_socket_flushed.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_socket_blocked",
            self.stream_write_socket_blocked.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_write_socket_errored",
            self.stream_write_socket_errored.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_flushed",
            self.stream_read_flushed.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_fin_flushed",
            self.stream_read_fin_flushed.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_blocked",
            self.stream_read_blocked.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_errored",
            self.stream_read_errored.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_shutdown",
            self.stream_read_shutdown.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_socket_flushed",
            self.stream_read_socket_flushed.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_socket_blocked",
            self.stream_read_socket_blocked.load(Ordering::Relaxed) as _,
        );
        self.recorder.increment_counter(
            "stream_read_socket_errored",
            self.stream_read_socket_errored.load(Ordering::Relaxed) as _,
        );
    }
}

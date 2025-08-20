// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// DO NOT MODIFY THIS FILE
// This file was generated with the `s2n-quic-events` crate and any required
// changes should be made there.

use crate::event::{self, api, metrics::Recorder};
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
    byte_array_event: u64,
    enum_event: u64,
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
        &mut self,
        meta: &api::ConnectionMeta,
        info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        Context {
            recorder: self.subscriber.create_connection_context(meta, info),
            byte_array_event: 0,
            enum_event: 0,
        }
    }
    #[inline]
    fn on_byte_array_event(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::ByteArrayEvent,
    ) {
        context.byte_array_event += 1;
        self.subscriber
            .on_byte_array_event(&mut context.recorder, meta, event);
    }
    #[inline]
    fn on_enum_event(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &api::ConnectionMeta,
        event: &api::EnumEvent,
    ) {
        context.enum_event += 1;
        self.subscriber
            .on_enum_event(&mut context.recorder, meta, event);
    }
}
impl<R: Recorder> Drop for Context<R> {
    fn drop(&mut self) {
        self.recorder
            .increment_counter("byte_array_event", self.byte_array_event as _);
        self.recorder
            .increment_counter("enum_event", self.enum_event as _);
    }
}

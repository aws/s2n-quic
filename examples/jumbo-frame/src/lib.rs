// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::event::Subscriber;

pub struct MtuEventInformer {}

pub struct MtuEventInformerContext {}

impl Subscriber for MtuEventInformer {
    type ConnectionContext = MtuEventInformerContext;

    fn create_connection_context(
        &mut self,
        _meta: &s2n_quic::provider::event::ConnectionMeta,
        _info: &s2n_quic::provider::event::ConnectionInfo,
    ) -> Self::ConnectionContext {
        MtuEventInformerContext {}
    }

    fn on_mtu_updated(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &s2n_quic::provider::event::ConnectionMeta,
        event: &s2n_quic::provider::event::events::MtuUpdated,
    ) {
        eprintln!("{:?}", event);
    }
}

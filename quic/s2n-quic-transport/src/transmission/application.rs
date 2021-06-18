// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    contexts::WriteContext,
    path,
    path::mtu,
    recovery,
    recovery::congestion_controller,
    space::{rx_packet_numbers::AckManager, HandshakeStatus},
    stream::{AbstractStreamManager, StreamTrait as Stream},
    sync::flag::Ping,
    transmission,
    transmission::{interest::Provider, Interest},
};
use core::ops::RangeInclusive;
use s2n_quic_core::packet::number::PacketNumberSpace;

pub enum Payload<'a, S: Stream, CCE: congestion_controller::Endpoint> {
    Normal(Normal<'a, S, CCE>),
    MtuProbe(MtuProbe<'a>),
}

impl<'a, S: Stream, CCE: congestion_controller::Endpoint> super::Payload for Payload<'a, S, CCE> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        match self {
            Payload::Normal(inner) => inner.size_hint(range),
            Payload::MtuProbe(inner) => inner.size_hint(range),
        }
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        match self {
            Payload::Normal(inner) => inner.on_transmit(context),
            Payload::MtuProbe(inner) => inner.on_transmit(context),
        }
    }

    fn packet_number_space(&self) -> PacketNumberSpace {
        PacketNumberSpace::ApplicationData
    }
}

impl<'a, S: Stream, CCE: congestion_controller::Endpoint> transmission::interest::Provider
    for Payload<'a, S, CCE>
{
    fn transmission_interest(&self) -> Interest {
        match self {
            Payload::Normal(inner) => inner.transmission_interest(),
            Payload::MtuProbe(inner) => inner.transmission_interest(),
        }
    }
}

pub struct Normal<'a, S: Stream, CCE: congestion_controller::Endpoint> {
    pub ack_manager: &'a mut AckManager,
    pub handshake_status: &'a mut HandshakeStatus,
    pub ping: &'a mut Ping,
    pub stream_manager: &'a mut AbstractStreamManager<S>,
    pub local_id_registry: &'a mut connection::LocalIdRegistry,
    pub path_manager: &'a mut path::Manager<CCE>,
    pub recovery_manager: &'a mut recovery::Manager,
}

impl<'a, S: Stream, CCE: congestion_controller::Endpoint> Normal<'a, S, CCE> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        // We need at least 1 byte to write a HANDSHAKE_DONE or PING frame
        (*range.start()).max(1)
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let did_send_ack = self.ack_manager.on_transmit(context);

        // Payloads can only transmit and retransmit
        if context.transmission_constraint().can_transmit()
            || context.transmission_constraint().can_retransmit()
        {
            // send HANDSHAKE_DONE frames first, if needed, to ensure the handshake is confirmed as
            // soon as possible
            let _ = self.handshake_status.on_transmit(context);

            self.local_id_registry.on_transmit(context);

            self.path_manager.on_transmit(context);

            let _ = self.stream_manager.on_transmit(context);

            // send PINGs last, since they might not actually be needed if there's an ack-eliciting
            // frame already present in the payload
            self.recovery_manager.on_transmit(context);
            let _ = self.ping.on_transmit(context);
        }

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(context);
        }
    }

    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.handshake_status.transmission_interest()
            + self.stream_manager.transmission_interest()
            + self.local_id_registry.transmission_interest()
            + self.path_manager.transmission_interest()
            + self.recovery_manager.transmission_interest()
    }
}

pub struct MtuProbe<'a> {
    pub mtu_controller: &'a mut mtu::Controller,
}

impl<'a> MtuProbe<'a> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        // MTU Probes use the full datagram
        *range.end()
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if context.transmission_constraint().can_transmit() {
            self.mtu_controller.on_transmit(context)
        }
    }

    fn transmission_interest(&self) -> transmission::Interest {
        self.mtu_controller.transmission_interest()
    }
}

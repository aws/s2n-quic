// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    contexts::WriteContext,
    path, recovery,
    recovery::congestion_controller,
    space::{rx_packet_numbers::AckManager, HandshakeStatus},
    stream::{AbstractStreamManager, StreamTrait as Stream},
    sync::flag::Ping,
    transmission,
};
use core::ops::RangeInclusive;
use s2n_quic_core::packet::number::PacketNumberSpace;

pub struct Payload<'a, S: Stream, CCE: congestion_controller::Endpoint> {
    pub ack_manager: &'a mut AckManager,
    pub handshake_status: &'a mut HandshakeStatus,
    pub ping: &'a mut Ping,
    pub stream_manager: &'a mut AbstractStreamManager<S>,
    pub local_id_registry: &'a mut connection::LocalIdRegistry,
    pub path_manager: &'a mut path::Manager<CCE>,
    pub recovery_manager: &'a mut recovery::Manager,
    pub path_id: path::id,
}

impl<'a, S: Stream, CCE: congestion_controller::Endpoint> super::Payload for Payload<'a, S, CCE> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        // We need at least 1 byte to write a HANDSHAKE_DONE or PING frame
        (*range.start()).max(1)
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let did_send_ack = self.ack_manager.on_transmit(context);


        // this can be simplified to only checking the active path
        (self.path_manager.requires_probing, self.path_manager.is_active_path(path_id))
            (_, false) => probe_on_transmit
            (_, true) => probe_on_transmit + app_on_transmit

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

    fn packet_number_space(&self) -> PacketNumberSpace {
        PacketNumberSpace::ApplicationData
    }
}

impl<'a, S: Stream, CCE: congestion_controller::Endpoint> transmission::interest::Provider
    for Payload<'a, S, CCE>
{
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

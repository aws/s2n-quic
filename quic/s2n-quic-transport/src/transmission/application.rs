// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    connection::ConnectionTransmissionContext,
    contexts::WriteContext,
    endpoint, path,
    path::mtu,
    recovery,
    recovery::congestion_controller,
    space::{rx_packet_numbers::AckManager, HandshakeStatus},
    stream::{AbstractStreamManager, StreamTrait as Stream},
    sync::{flag, flag::Ping},
    transmission,
    transmission::{interest::Provider, Interest, Mode},
};
use core::ops::RangeInclusive;
use s2n_quic_core::packet::number::PacketNumberSpace;

pub enum Payload<'a, Config: endpoint::Config> {
    Normal(Normal<'a, Config::Stream, Config::CongestionControllerEndpoint>),
    MtuProbe(MtuProbe<'a>),
    PathValidation(PathValidation<'a, Config::CongestionControllerEndpoint>),
}

impl<'a, Config: endpoint::Config> Payload<'a, Config> {
    /// Constructs a transmission::application::Payload appropriate for the given
    /// `transmission::Mode` in the given `ConnectionTransmissionContext`
    pub fn new(
        context: &'a mut ConnectionTransmissionContext<Config>,
        ack_manager: &'a mut AckManager,
        handshake_status: &'a mut HandshakeStatus,
        ping: &'a mut flag::Ping,
        stream_manager: &'a mut AbstractStreamManager<Config::Stream>,
        recovery_manager: &'a mut recovery::Manager,
    ) -> Self {
        match context.transmission_mode {
            Mode::LossRecoveryProbing | Mode::Normal => {
                transmission::application::Payload::Normal(Normal {
                    ack_manager,
                    handshake_status,
                    ping,
                    stream_manager,
                    local_id_registry: context.local_id_registry,
                    path_manager: context.path_manager,
                    recovery_manager,
                    path_id: context.path_id,
                })
            }
            Mode::MtuProbing => transmission::application::Payload::MtuProbe(MtuProbe {
                mtu_controller: &mut context.path_mut().mtu_controller,
            }),
            Mode::PathValidation => {
                transmission::application::Payload::PathValidation(PathValidation {
                    path_manager: context.path_manager,
                    path_id: context.path_id,
                })
            }
        }
    }
}

impl<'a, Config: endpoint::Config> super::Payload for Payload<'a, Config> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        // We need at least 1 byte to write a HANDSHAKE_DONE or PING frame
        (*range.start()).max(1)
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        match self {
            Payload::Normal(inner) => inner.on_transmit(context),
            Payload::MtuProbe(inner) => inner.on_transmit(context),
            Payload::PathValidation(inner) => inner.on_transmit(context),
        }
    }

    fn packet_number_space(&self) -> PacketNumberSpace {
        PacketNumberSpace::ApplicationData
    }
}

impl<'a, Config: endpoint::Config> transmission::interest::Provider for Payload<'a, Config> {
    fn transmission_interest(&self) -> Interest {
        match self {
            Payload::Normal(inner) => inner.transmission_interest(),
            Payload::MtuProbe(inner) => inner.transmission_interest(),
            Payload::PathValidation(inner) => inner.transmission_interest(),
        }
    }
}

pub struct Normal<'a, S: Stream, CCE: congestion_controller::Endpoint> {
    ack_manager: &'a mut AckManager,
    handshake_status: &'a mut HandshakeStatus,
    ping: &'a mut Ping,
    stream_manager: &'a mut AbstractStreamManager<S>,
    local_id_registry: &'a mut connection::LocalIdRegistry,
    path_manager: &'a mut path::Manager<CCE>,
    recovery_manager: &'a mut recovery::Manager,
    path_id: path::Id,
}

impl<'a, S: Stream, CCE: congestion_controller::Endpoint> Normal<'a, S, CCE> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let did_send_ack = self.ack_manager.on_transmit(context);

        if !self.path_manager[self.path_id].at_amplification_limit() {
            self.path_manager[self.path_id].on_transmit(context)
        }

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
            + self.path_manager[self.path_id].transmission_interest()
    }
}

pub struct MtuProbe<'a> {
    mtu_controller: &'a mut mtu::Controller,
}

impl<'a> MtuProbe<'a> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if context.transmission_constraint().can_transmit() {
            self.mtu_controller.on_transmit(context)
        }
    }

    fn transmission_interest(&self) -> transmission::Interest {
        self.mtu_controller.transmission_interest()
    }
}

pub struct PathValidation<'a, CCE: congestion_controller::Endpoint> {
    path_manager: &'a mut path::Manager<CCE>,
    path_id: path::Id,
}

impl<'a, CCE: congestion_controller::Endpoint> PathValidation<'a, CCE> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if !self.path_manager[self.path_id].at_amplification_limit() {
            self.path_manager[self.path_id].on_transmit(context)
        }
    }

    fn transmission_interest(&self) -> transmission::Interest {
        self.path_manager[self.path_id].transmission_interest()
    }
}

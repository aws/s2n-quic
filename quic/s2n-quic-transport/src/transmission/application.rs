// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack::AckManager,
    connection,
    contexts::WriteContext,
    endpoint, path,
    path::mtu,
    recovery,
    space::{datagram, HandshakeStatus},
    stream::{AbstractStreamManager, StreamTrait as Stream},
    sync::{flag, flag::Ping},
    transmission::{self, Mode},
};
use core::ops::RangeInclusive;
use s2n_quic_core::packet::number::PacketNumberSpace;

pub enum Payload<'a, Config: endpoint::Config> {
    Normal(Normal<'a, Config::Stream, Config>),
    MtuProbe(MtuProbe<'a>),
    /// For use on non-active paths where only path validation frames are sent.
    PathValidationOnly(PathValidationOnly<'a, Config>),
}

impl<'a, Config: endpoint::Config> Payload<'a, Config> {
    /// Constructs a transmission::application::Payload appropriate for the given
    /// `transmission::Mode` in the given `ConnectionTransmissionContext`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path_id: path::Id,
        path_manager: &'a mut path::Manager<Config>,
        local_id_registry: &'a mut connection::LocalIdRegistry,
        transmission_mode: transmission::Mode,
        ack_manager: &'a mut AckManager,
        handshake_status: &'a mut HandshakeStatus,
        ping: &'a mut flag::Ping,
        stream_manager: &'a mut AbstractStreamManager<Config::Stream>,
        recovery_manager: &'a mut recovery::Manager<Config>,
        datagram_manager: &'a mut datagram::Manager<Config>,
    ) -> Self {
        if transmission_mode != Mode::PathValidationOnly {
            debug_assert_eq!(path_id, path_manager.active_path_id());
        }

        match transmission_mode {
            Mode::LossRecoveryProbing | Mode::Normal => {
                transmission::application::Payload::Normal(Normal {
                    ack_manager,
                    handshake_status,
                    ping,
                    stream_manager,
                    local_id_registry,
                    path_manager,
                    recovery_manager,
                    datagram_manager,
                    prioritize_datagrams: false,
                })
            }
            Mode::MtuProbing => transmission::application::Payload::MtuProbe(MtuProbe {
                mtu_controller: &mut path_manager[path_id].mtu_controller,
            }),
            Mode::PathValidationOnly => {
                transmission::application::Payload::PathValidationOnly(PathValidationOnly {
                    path: &mut path_manager[path_id],
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
            Payload::PathValidationOnly(inner) => inner.on_transmit(context),
        }
    }

    fn packet_number_space(&self) -> PacketNumberSpace {
        PacketNumberSpace::ApplicationData
    }
}

impl<'a, Config: endpoint::Config> transmission::interest::Provider for Payload<'a, Config> {
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        match self {
            Payload::Normal(inner) => inner.transmission_interest(query),
            Payload::MtuProbe(inner) => inner.transmission_interest(query),
            Payload::PathValidationOnly(inner) => inner.transmission_interest(query),
        }
    }
}

pub struct Normal<'a, S: Stream, Config: endpoint::Config> {
    ack_manager: &'a mut AckManager,
    handshake_status: &'a mut HandshakeStatus,
    ping: &'a mut Ping,
    stream_manager: &'a mut AbstractStreamManager<S>,
    local_id_registry: &'a mut connection::LocalIdRegistry,
    path_manager: &'a mut path::Manager<Config>,
    recovery_manager: &'a mut recovery::Manager<Config>,
    datagram_manager: &'a mut datagram::Manager<Config>,
    prioritize_datagrams: bool,
}

impl<'a, S: Stream, Config: endpoint::Config> Normal<'a, S, Config> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let can_transmit = context.transmission_constraint().can_transmit()
            || context.transmission_constraint().can_retransmit();

        //= https://www.rfc-editor.org/rfc/rfc9221#section-5
        //# DATAGRAM frames cannot be fragmented;
        //
        // We alternate between prioritizing filling the packet with datagrams
        // and filling the packet with other frames. This is because datagrams
        // cannot be fragmented across packets and we want to do the most to send
        // large datagrams.
        if self.prioritize_datagrams && can_transmit {
            self.datagram_manager
                .on_transmit(context, self.stream_manager);
        }
        let did_send_ack = self.ack_manager.on_transmit(context);

        // Payloads can only transmit and retransmit
        if can_transmit {
            self.transmit_control_data(context);

            // If we did not prioritize datagrams in this packet, we send them just
            // before we send stream data.
            if !self.prioritize_datagrams {
                self.datagram_manager
                    .on_transmit(context, self.stream_manager);
            }

            // The default sending behavior is to alternate between sending datagrams
            // and sending stream data. This can be configured by implementing a
            // custom datagram sender and choosing when to cede packet space for stream data.
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

        // Alternate between prioritizing datagrams or not each packet
        self.prioritize_datagrams = !self.prioritize_datagrams;
    }

    // Sends control data frames
    fn transmit_control_data<W: WriteContext>(&mut self, context: &mut W) {
        // send HANDSHAKE_DONE frames first, if needed, to ensure the handshake is confirmed as
        // soon as possible
        self.handshake_status.on_transmit(context);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2
        //# An endpoint MAY include other frames with the PATH_CHALLENGE and
        //# PATH_RESPONSE frames used for path validation.
        // prioritize PATH_CHALLENGE and PATH_RESPONSE frames higher than app data
        self.path_manager.active_path_mut().on_transmit(context);

        self.local_id_registry.on_transmit(context);

        self.path_manager.on_transmit(context);
    }
}

impl<'a, S: Stream, Config: endpoint::Config> transmission::interest::Provider
    for Normal<'a, S, Config>
{
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.ack_manager.transmission_interest(query)?;
        self.handshake_status.transmission_interest(query)?;
        self.stream_manager.transmission_interest(query)?;
        self.datagram_manager.transmission_interest(query)?;
        self.local_id_registry.transmission_interest(query)?;
        self.path_manager.transmission_interest(query)?;
        self.recovery_manager.transmission_interest(query)?;
        self.path_manager
            .active_path()
            .transmission_interest(query)?;
        self.ping.transmission_interest(query)?;
        Ok(())
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
}

impl<'a> transmission::interest::Provider for MtuProbe<'a> {
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.mtu_controller.transmission_interest(query)
    }
}

pub struct PathValidationOnly<'a, Config: endpoint::Config> {
    path: &'a mut path::Path<Config>,
}

impl<'a, Config: endpoint::Config> PathValidationOnly<'a, Config> {
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        if context.transmission_constraint().can_transmit() {
            self.path.on_transmit(context)
        }
    }
}

impl<'a, Config: endpoint::Config> transmission::interest::Provider
    for PathValidationOnly<'a, Config>
{
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.path.transmission_interest(query)
    }
}

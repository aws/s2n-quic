use super::{generator::gen_ack_settings, Packet, TestEnvironment};
use crate::{
    contexts::WriteContext,
    frame_exchange_interests::FrameExchangeInterestProvider,
    processed_packet::ProcessedPacket,
    space::rx_packet_numbers::{ack_manager::AckManager, ack_ranges::DEFAULT_ACK_RANGES_LIMIT},
};
use bolero::generator::*;
use s2n_quic_core::{
    endpoint::EndpointType,
    frame::{ack_elicitation::AckElicitation, Ack, Frame, Ping},
    inet::DatagramInfo,
    packet::number::PacketNumberSpace,
    time::Timestamp,
    transport::parameters::AckSettings,
};

#[derive(Clone, Debug, TypeGenerator)]
pub struct Endpoint {
    #[generator(constant(TestEnvironment::new()))]
    pub env: TestEnvironment,

    #[generator(gen_ack_settings().map(new_ack_manager))]
    pub ack_manager: AckManager,
}

fn new_ack_manager(ack_settings: AckSettings) -> AckManager {
    AckManager::new(
        PacketNumberSpace::ApplicationData,
        ack_settings,
        DEFAULT_ACK_RANGES_LIMIT,
    )
}

impl Endpoint {
    pub fn new(ack_settings: AckSettings) -> Self {
        Self {
            env: TestEnvironment::new(),
            ack_manager: new_ack_manager(ack_settings),
        }
    }

    pub fn init(&mut self, now: Timestamp, endpoint_type: EndpointType) {
        self.env.current_time = now;
        self.env.connection_context.local_endpoint_type = endpoint_type;
    }

    pub fn recv(&mut self, packet: Packet) {
        self.env.current_time = packet.time;

        self.ack_manager.on_timeout(self.env.current_time);

        let datagram = DatagramInfo {
            ecn: packet.ecn,
            payload_len: 1200,
            remote_address: Default::default(),
            timestamp: self.env.current_time,
        };

        if let Some(ack) = packet.ack {
            for ack_range in ack.ack_ranges {
                self.ack_manager.on_packet_ack(&datagram, &ack_range);
            }
        }

        let packet = ProcessedPacket {
            ack_elicitation: packet.ack_elicitation,
            datagram: &datagram,
            packet_number: packet.packet_number,
        };

        self.ack_manager.on_processed_packet(&packet);
    }

    pub fn send(&mut self, now: Timestamp) -> Option<Packet> {
        self.env.current_time = now;
        self.ack_manager.on_timeout(now);
        self.transmit(AckElicitation::Eliciting)
    }

    pub fn tick(&mut self, now: Timestamp) -> Option<Packet> {
        self.env.current_time = now;
        self.ack_manager.on_timeout(now);

        if !self.ack_manager.frame_exchange_interests().transmission {
            return None;
        }

        self.transmit(AckElicitation::NonEliciting)
    }

    pub fn next_tick(&self) -> Option<Timestamp> {
        self.timers().min().cloned()
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.ack_manager.timers()
    }

    fn transmit(&mut self, ack_elicitation: AckElicitation) -> Option<Packet> {
        let mut context = self.env.context();
        let did_send_ack = self.ack_manager.on_transmit(&mut context);

        if ack_elicitation.is_ack_eliciting() {
            context.write_frame(&Ping);
        }

        if did_send_ack {
            self.ack_manager.on_transmit_complete(&mut context);
        }

        let ack_elicitation = context.ack_elicitation();
        let packet_number = context.packet_number();

        if self.env.sent_frames.is_empty() {
            return None;
        }

        let mut packet = Packet {
            packet_number,
            ack_elicitation,
            ecn: Default::default(),
            time: self.env.current_time,
            ack: None,
        };

        while let Some(mut frame) = self.env.sent_frames.pop_front() {
            if let Frame::Ack(ack) = frame.as_frame() {
                packet.ack = Some(Ack {
                    ack_delay: ack.ack_delay,
                    ecn_counts: ack.ecn_counts,
                    ack_ranges: ack
                        .ack_ranges()
                        .map(|ack_range| {
                            let (start, end) = ack_range.into_inner();

                            let pn_space = PacketNumberSpace::ApplicationData;
                            let start = pn_space.new_packet_number(start);
                            let end = pn_space.new_packet_number(end);

                            start..=end
                        })
                        .collect(),
                });
            }
        }

        self.env.sent_frames.flush();

        Some(packet)
    }

    pub fn done(&mut self) {
        assert!(
            !self.ack_manager.frame_exchange_interests().transmission,
            "ack manager should be in a stable state"
        );
    }
}

use super::{
    generator::{gen_duration, gen_ecn},
    EndpointReport, Packet,
};
use bolero::generator::*;
use core::time::Duration;
use s2n_quic_core::inet::ExplicitCongestionNotification;

#[derive(Clone, Copy, Debug, TypeGenerator)]
pub enum NetworkEvent {
    Pass,
    Drop,
    Ecn(#[generator(gen_ecn())] ExplicitCongestionNotification),
    Delay(#[generator(gen_duration())] Duration),
}

impl NetworkEvent {
    pub fn process_packet(
        &self,
        mut packet: Packet,
        report: &mut EndpointReport,
    ) -> Option<Packet> {
        report.total_transmissions += 1;

        match self {
            NetworkEvent::Drop => {
                report.dropped_transmissions += 1;
                return None;
            }
            NetworkEvent::Pass => {}
            NetworkEvent::Delay(delay) => {
                packet.time += *delay;
                report.delayed_transmissions += 1;
            }
            NetworkEvent::Ecn(ecn) => {
                packet.ecn = *ecn;
                report.congested_transmissions += 1;
            }
        }

        report.processed_transmissions += 1;

        if packet.ack_elicitation.is_ack_eliciting() {
            report.ack_eliciting_transmissions += 1;
        }

        if packet.ack.is_some() {
            report.ack_transmissions += 1;
        }

        Some(packet)
    }
}

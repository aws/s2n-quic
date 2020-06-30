#![allow(dead_code)]

use crate::space::rx_packet_numbers::ack_eliciting_transmission::AckElicitingTransmission;
use s2n_quic_core::{
    packet::number::{PacketNumber, PacketNumberSpace},
    varint::VarInt,
};

/// Generates AckElicitingTransmissions from increasing packet numbers
pub fn transmissions_iter() -> impl Iterator<Item = AckElicitingTransmission> {
    packet_numbers_iter().map(|pn| AckElicitingTransmission {
        sent_in_packet: pn,
        largest_received_packet_number_acked: pn,
    })
}

/// Generates increasing packet numbers
pub fn packet_numbers_iter() -> impl Iterator<Item = PacketNumber> {
    Iterator::map(0u32.., |pn| {
        PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u32(pn))
    })
}

pub mod application;
pub mod endpoint;
pub mod environment;
pub mod generator;
pub mod network;
pub mod network_event;
pub mod network_interface;
pub mod packet;
pub mod report;
pub mod simulation;

pub use application::*;
pub use endpoint::*;
pub use environment::*;
pub use network::*;
pub use network_event::*;
pub use network_interface::*;
pub use packet::*;
pub use report::*;
pub use simulation::*;

#[test]
fn simulation_harness_test() {
    use core::time::Duration;
    use s2n_quic_core::transport::parameters::AckSettings;

    let client_transmissions = 100;
    let server_transmissions = 10;

    let mut simulation = Simulation {
        network: Network {
            client: Application::new(
                Endpoint::new(AckSettings {
                    max_ack_delay: Duration::from_millis(25),
                    ack_delay_exponent: 1,
                }),
                // send an ack-eliciting packet every 5ms, 100 times
                [Duration::from_millis(5)]
                    .iter()
                    .cycle()
                    .take(client_transmissions)
                    .cloned(),
            )
            .into(),
            server: Application::new(
                Endpoint::new(AckSettings {
                    max_ack_delay: Duration::from_millis(25),
                    ack_delay_exponent: 1,
                }),
                // send an ack-eliciting packet every 800ms, 10 times
                [Duration::from_millis(800)]
                    .iter()
                    .cycle()
                    .take(server_transmissions)
                    .cloned(),
            )
            .into(),
        },
        // pass all packets unchanged
        events: [NetworkEvent::Pass].iter().cloned().collect(),
        // delay sending each packet by 100ms
        delay: Duration::from_millis(100),
    };

    let report = simulation.run();

    assert!(report.client.ack_eliciting_transmissions >= client_transmissions);
    assert!(report.client.dropped_transmissions == 0);

    assert!(report.server.ack_eliciting_transmissions >= server_transmissions);
    assert!(report.server.dropped_transmissions == 0);
}

/// Ack Manager Simulation
///
/// This target will generate and execute fuzz-guided simulation scenarios.
///
/// The following assertions are made:
///
/// * The program doesn't crash
///
/// * Two AckManagers talking to each other in various configurations
///   and network scenarios will always terminate (not endlessly ACK each other)
///
/// Additional checks may be implemented at some point to expand guarantees
#[test]
fn simulation_test() {
    bolero::fuzz!()
        .with_type::<Simulation>()
        .cloned()
        .for_each(|mut simulation| {
            let _report = simulation.run();

            // TODO make assertions about report
        });
}

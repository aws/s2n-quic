// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::ack::testing::packet_numbers_iter;

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
    use s2n_quic_core::ack;

    let client_transmissions = 100;
    let server_transmissions = 10;

    let mut simulation = Simulation {
        network: Network {
            client: Application::new(
                Endpoint::new(ack::Settings {
                    max_ack_delay: Duration::from_millis(25),
                    ack_delay_exponent: 1,
                    ..Default::default()
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
                Endpoint::new(ack::Settings {
                    max_ack_delay: Duration::from_millis(25),
                    ack_delay_exponent: 1,
                    ..Default::default()
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
    bolero::check!()
        .with_type::<Simulation>()
        .cloned()
        .for_each(|mut simulation| {
            let _report = simulation.run();

            // TODO make assertions about report
        });
}

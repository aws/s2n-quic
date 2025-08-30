// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::encoder::scatter;
use s2n_quic::provider::tls;
use s2n_quic_core::{
    event::api::Subject,
    packet::interceptor::{Interceptor, Packet},
    path::{mtu, BaseMtu, InitialMtu},
};

macro_rules! mtu_test {
    (fn $name:ident($server:ident, $client:ident) $impl:block) => {
        mod $name {
            use super::*;

            #[test]
            fn one_way_auth() {
                let $client = certificates::CERT_PEM;
                let $server = SERVER_CERTS;
                $impl
            }

            #[cfg(not(target_os = "windows"))]
            #[test]
            fn mutual_auth() {
                let $client = build_client_mtls_provider(certificates::MTLS_CA_CERT).unwrap();
                let $server = build_server_mtls_provider(certificates::MTLS_CA_CERT).unwrap();
                $impl
            }
        }
    };
}

struct CustomMtu(mtu::Config);

impl mtu::Endpoint for CustomMtu {
    fn on_path(
        &mut self,
        _info: &mtu::PathInfo,
        _endpoint_mtu_config: mtu::Config,
    ) -> Option<mtu::Config> {
        Some(self.0)
    }
}

// Construct a simulation where a client sends some data, which the server echos
// back. The MtuUpdated events that the server experiences are recorded and
// returns at the end of the simulation.
fn mtu_updates<S: tls::Provider, C: tls::Provider>(
    server: S,
    client: C,
    config_override: Option<mtu::Config>,
    initial_mtu: u16,
    base_mtu: u16,
    max_mtu: u16,
    network_max_udp_payload: u16,
) -> Vec<events::MtuUpdated> {
    let model = Model::default();
    model.set_max_udp_payload(network_max_udp_payload);

    let subscriber = recorder::MtuUpdated::new();
    let events = subscriber.events();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(
                handle
                    .builder()
                    .with_max_mtu(max_mtu)
                    .with_initial_mtu(initial_mtu)
                    .with_base_mtu(base_mtu)
                    .build()?,
            )?
            .with_tls(server)?
            .with_event((tracing_events(), subscriber))?
            .with_random(Random::with_seed(456))?;

        let server = if let Some(config_override) = config_override {
            server.with_mtu(CustomMtu(config_override))?.start()?
        } else {
            server.start()?
        };

        let client = Client::builder()
            .with_io(
                handle
                    .builder()
                    .with_max_mtu(max_mtu)
                    .with_initial_mtu(initial_mtu)
                    .with_base_mtu(base_mtu)
                    .build()
                    .unwrap(),
            )?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .with_tls(client)?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(50_000_000))?;
        Ok(addr)
    })
    .unwrap();

    let events_handle = events.lock().unwrap();
    events_handle.clone()
}

// if we specify jumbo frames on the endpoint and the network supports them,
// then jumbo frames should be negotiated.
mtu_test!(
    fn mtu_probe_jumbo_frame_test(server, client) {
        let events = mtu_updates(
            server,
            client,
            None,
            InitialMtu::default().into(),
            BaseMtu::default().into(),
            9_001,
            10_000,
        );

        // handshake is padded to 1200, so we should immediately have an mtu of 1200
        // since the handshake successfully completes
        let handshake_mtu = events[0].clone();
        assert_eq!(handshake_mtu.mtu, 1200);
        assert!(matches!(
            handshake_mtu.cause,
            events::MtuUpdatedCause::NewPath { .. }
        ));

        // we should then successfully probe for 1500 (minus headers = 1472)
        let first_probe = events[1].clone();
        assert_eq!(first_probe.mtu, 1472);
        assert!(!first_probe.search_complete);
        assert!(matches!(
            first_probe.cause,
            events::MtuUpdatedCause::ProbeAcknowledged { .. }
        ));

        // we binary search upwards 9001
        // this isn't the maximum mtu we'd find in practice, just the maximum mtu we
        // find with a payload of 10_000_000 bytes.
        let last_probe = events.last().unwrap();
        assert_eq!(last_probe.mtu, 8943);
        assert!(last_probe.search_complete);
        assert!(matches!(
            last_probe.cause,
            events::MtuUpdatedCause::ProbeAcknowledged { .. }
        ));
    }
);

// if we specify jumbo frames on the endpoint and the network does not support
// them, the connection should gracefully complete with a smaller mtu
mtu_test!(
    fn mtu_probe_jumbo_frame_unsupported_test(server, client) {
        let events = mtu_updates(
            server,
            client,
            None,
            InitialMtu::default().into(),
            BaseMtu::default().into(),
            9_001,
            1472,
        );
        let last_mtu = events.last().unwrap();
        // ETHERNET_MTU - UDP_HEADER_LEN - IPV4_HEADER_LEN
        assert_eq!(last_mtu.mtu, 1472);
        assert!(last_mtu.search_complete);
        assert!(matches!(
            last_mtu.cause,
            events::MtuUpdatedCause::LargerProbesLost { .. }
        ));
    }
);

// The network can support the maximum sized UDP packet
mtu_test!(
    fn mtu_probe_max_jumbo_frame_test(server, client) {
        let events = mtu_updates(
            server,
            client,
            None,
            InitialMtu::default().into(),
            BaseMtu::default().into(),
            u16::MAX,
            u16::MAX,
        );
        let last_mtu = events.last().unwrap();
        assert_eq!(last_mtu.mtu, 65475);
        assert!(last_mtu.search_complete);
        assert!(matches!(
            last_mtu.cause,
            events::MtuUpdatedCause::ProbeAcknowledged { .. }
        ));
    }
);

// The configured base mtu is the smallest MTU used
mtu_test!(
    fn base_mtu(server, client) {
        let events = mtu_updates(server, client, None, 1250, 1250, 9_001, 10_000);
        let base_mtu = events
            .iter()
            .min_by_key(|&mtu_event| mtu_event.mtu)
            .unwrap();
        // 1250 - UDP_HEADER_LEN - IPV4_HEADER_LEN
        assert_eq!(base_mtu.mtu, 1222);
        assert!(!base_mtu.search_complete);
    }
);

// The configured initial mtu is the first MTU used
mtu_test!(
    fn initial_mtu(server, client) {
        let events = mtu_updates(server, client, None, 2000, BaseMtu::default().into(), 9_001, 10_000);
        let first_mtu = events.first().unwrap();
        // 2000 - UDP_HEADER_LEN - IPV4_HEADER_LEN
        assert_eq!(first_mtu.mtu, 1972);
        assert!(!first_mtu.search_complete);
    }
);

// No override config
mtu_test!(
    fn conn_mtu_no_override(server, client) {
        let events = mtu_updates(server, client, None, 1228, BaseMtu::default().into(), 9_001, 10_000);
        let first_mtu = events.first().unwrap();
        // 1228 - UDP_HEADER_LEN - IPV4_HEADER_LEN
        assert_eq!(first_mtu.mtu, 1200);
        assert!(!first_mtu.search_complete);
        let last_mtu = events.last().unwrap();
        assert_eq!(last_mtu.mtu, 8943);
        assert!(last_mtu.search_complete);
    }
);

// Connection specific initial MTU
//
// Override the initial (first) MTU we can use for the connection.
mtu_test!(
    fn conn_mtu_initial_with_override(server, client) {
        let config = mtu::Config::builder()
            .with_initial_mtu(1328)
            .unwrap()
            .build()
            .unwrap();
        let events = mtu_updates(server, client, Some(config), 1228, BaseMtu::default().into(), 9_001, 10_000);
        let first_mtu = events.first().unwrap();
        // 1528 - UDP_HEADER_LEN - IPV4_HEADER_LEN
        assert_eq!(first_mtu.mtu, 1300);
        assert!(!first_mtu.search_complete);
    }
);

// Connection specific base MTU
//
// Override the base (lowest) MTU we can use for the connection.
mtu_test!(
    fn conn_mtu_base(server, client) {
        let config = mtu::Config::builder()
            .with_base_mtu(1328)
            .unwrap()
            .build()
            .unwrap();
        let events = mtu_updates(
            server,
            client,
            Some(config),
            1228,
            BaseMtu::default().into(),
            9_001,
            10_000,
        );
        let first_mtu = events.first().unwrap();
        assert_eq!(first_mtu.mtu, 1300);
        assert!(!first_mtu.search_complete);
    }
);

// Connection specific max MTU
//
// Override the max MTU we can use for the connection.
mtu_test!(
    fn conn_mtu_max(server, client) {
        let config = mtu::Config::builder()
            .with_max_mtu(6_000)
            .unwrap()
            .build()
            .unwrap();
        let events = mtu_updates(server, client, Some(config), 1228, BaseMtu::default().into(), 9_001, 10_000);
        let last_mtu = events.last().unwrap();
        assert_eq!(last_mtu.mtu, 5_936);
        assert!(last_mtu.search_complete);
    }
);

// The configured initial mtu is the first MTU used. It is not supported by the network, so
// the MTU drops to the base MTU, before increasing back to what the network supports.
mtu_test!(
    fn initial_mtu_not_supported(server, client) {
        let events = mtu_updates(server, client, None, 2000, BaseMtu::default().into(), 9_001, 1500);
        let first_mtu = events.first().unwrap();
        let second_mtu = events.get(1).unwrap();
        let last_mtu = events.last().unwrap();
        // First try the initial MTU
        assert_eq!(first_mtu.mtu, 1972);
        assert!(!first_mtu.search_complete);
        // Next drop down to the base MTU
        assert_eq!(second_mtu.mtu, 1200);
        assert!(!second_mtu.search_complete);
        assert!(matches!(
            second_mtu.cause,
            events::MtuUpdatedCause::InitialMtuPacketLost { .. }
        ));
        // Eventually reach the MTU the network supports
        assert_eq!(last_mtu.mtu, 1500);
        assert!(last_mtu.search_complete);
        assert!(matches!(
            last_mtu.cause,
            events::MtuUpdatedCause::ProbeAcknowledged { .. }
        ));
    }
);

// The configured initial MTU is jumbo and the network supports it.
mtu_test!(
    fn initial_mtu_is_jumbo(server, client) {
        let events = mtu_updates(server, client, None, 9_001, BaseMtu::default().into(), 9_001, 10_000);
        let first_mtu = events.first().unwrap();
        let last_mtu = events.last().unwrap();
        // First try the initial MTU
        assert_eq!(first_mtu.mtu, 8973);
        assert!(matches!(
            first_mtu.cause,
            events::MtuUpdatedCause::NewPath { .. }
        ));
        assert!(!first_mtu.search_complete);
        // Stay on this MTU since the network supports it
        assert_eq!(last_mtu.mtu, 8973);
        assert!(last_mtu.search_complete);
        assert!(matches!(
            last_mtu.cause,
            events::MtuUpdatedCause::InitialMtuPacketAcknowledged { .. }
        ));
    }
);

// The configured initial MTU is jumbo and the network does not support it. The configured minimum
// MTU is used next.
mtu_test!(
    fn initial_mtu_is_jumbo_not_supported(server, client) {
        let events = mtu_updates(server, client, None, 9_001, 1_500, 9_001, 2_500);
        let first_mtu = events.first().unwrap();
        let second_mtu = events.get(1).unwrap();
        let last_mtu = events.last().unwrap();
        // First try the initial MTU
        assert_eq!(first_mtu.mtu, 8_973);
        assert!(!first_mtu.search_complete);
        assert!(matches!(
            first_mtu.cause,
            events::MtuUpdatedCause::NewPath { .. }
        ));
        // Next drop down to the base MTU
        assert_eq!(second_mtu.mtu, 1472);
        assert!(!second_mtu.search_complete);
        assert!(matches!(
            second_mtu.cause,
            events::MtuUpdatedCause::InitialMtuPacketLost { .. }
        ));
        // Eventually reach the MTU the network supports
        assert_eq!(last_mtu.mtu, 2_496);
        assert!(last_mtu.search_complete);
        assert!(matches!(
            last_mtu.cause,
            events::MtuUpdatedCause::ProbeAcknowledged { .. }
        ));

    }
);

// MTU probing has been disabled by setting BaseMTU = InitialMTU = MaxMTU
mtu_test!(
    fn mtu_probing_disabled(server, client) {
        let events = mtu_updates(server, client, None, BaseMtu::default().into(), BaseMtu::default().into(), BaseMtu::default().into(), 10_000);
        let first_mtu = events.first().unwrap();
        assert_eq!(first_mtu.mtu, 1200);
        assert!(first_mtu.search_complete);
        assert!(matches!(
            first_mtu.cause,
            events::MtuUpdatedCause::NewPath { .. }
        ));
    }
);

// if we lose every packet during a round trip and then allow packets through,
// this is not determined to be an MTU black hole
#[test]
fn mtu_loss_no_blackhole() {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    let max_mtu = 9001;
    let subscriber = recorder::MtuUpdated::new();
    let events = subscriber.events();

    model.set_delay(rtt / 2);
    model.set_max_udp_payload(max_mtu);

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(10_000_000))?;

        spawn(async move {
            // let all packets go through for 10 RTTs - this will reach the end of MTU probing
            model.set_drop_rate(0.0);
            delay(rtt * 10).await;

            // drop all packets for a single round trip
            model.set_drop_rate(1.0);
            delay(rtt * 1).await;

            // now let the rest of the packets through
            model.set_drop_rate(0.0);
        });

        Ok(addr)
    })
    .unwrap();

    let events = events.lock().unwrap().clone();

    // MTU remained jumbo despite the packet loss
    let last_mtu = events.last().unwrap();
    assert_eq!(8943, last_mtu.mtu);
    assert!(last_mtu.search_complete);
}

// if the MTU is decreased after an MTU probe previously raised the MTU for the path,
// we detect an MTU black hole and decrease the MTU to the minimum
#[test]
fn mtu_blackhole() {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    let max_mtu = 9001;
    let subscriber = recorder::MtuUpdated::new();
    let events = subscriber.events();

    model.set_delay(rtt / 2);
    model.set_max_udp_payload(max_mtu);

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(10_000_000))?;

        spawn(async move {
            // let all packets go through for 10 RTTs - this will reach the end of MTU probing
            model.set_drop_rate(0.0);
            delay(rtt * 10).await;

            // decrease the MTU to trigger a blackhole
            model.set_max_udp_payload(1200);
        });

        Ok(addr)
    })
    .unwrap();

    let events = events.lock().unwrap().clone();

    // MTU dropped to the minimum
    let last_mtu = events.last().unwrap();
    assert_eq!(1200, last_mtu.mtu);
    assert!(last_mtu.search_complete);
    assert!(matches!(
        last_mtu.cause,
        events::MtuUpdatedCause::Blackhole { .. }
    ));
}

// ensure the server enforces the minimum MTU for all initial packets
#[test]
fn minimum_initial_packet() {
    let model = Model::default();
    let subscriber = recorder::PacketDropped::new();
    let drop_events = subscriber.events();

    let rtt = Duration::from_millis(100);
    model.set_delay(rtt / 2);

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_packet_interceptor((EraseClientHello, TruncatePadding))?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1_000))?;

        spawn(async move {
            delay(rtt / 4).await;

            // drop the server's initial ACK
            model.set_drop_rate(1.0);
            delay(rtt / 2).await;

            // let everything go through now
            model.set_drop_rate(0.0);
        });

        Ok(addr)
    })
    .unwrap();

    assert_eq!(
        drop_events.lock().unwrap().as_slice(),
        &[
            recorder::PacketDropReason::UndersizedInitialPacket,
            recorder::PacketDropReason::UndersizedInitialPacket,
        ]
    );
}

/// Erase client hello
struct EraseClientHello;

impl Interceptor for EraseClientHello {
    #[inline]
    fn intercept_tx_payload(
        &mut self,
        _subject: &Subject,
        packet: &Packet,
        payload: &mut scatter::Buffer,
    ) {
        if packet.number.space().is_initial() && packet.number.as_u64() == 0 {
            let payload = payload.flatten().as_mut_slice();
            payload.fill(0);
            payload[0] = 1;
        }
    }
}

/// Truncates paddings
struct TruncatePadding;

impl Interceptor for TruncatePadding {
    #[inline]
    fn intercept_tx_payload(
        &mut self,
        _subject: &Subject,
        packet: &Packet,
        payload: &mut scatter::Buffer,
    ) {
        if !(packet.number.space().is_initial() && (1..=4).contains(&packet.number.as_u64())) {
            return;
        }

        let buffer = payload.flatten();

        let mut pos = None;

        for (idx, v) in buffer.as_mut_slice().iter().copied().enumerate().rev() {
            if v == 0 && idx > 16 {
                continue;
            }

            pos = Some(idx + 1);
            break;
        }

        if let Some(pos) = pos {
            buffer.set_position(pos);
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

// Construct a simulation where a client sends some data, which the server echos
// back. The MtuUpdated events that the server experiences are recorded and
// returns at the end of the simulation.
fn mtu_updates(max_mtu: u16) -> Vec<events::MtuUpdated> {
    let model = Model::default();
    model.set_max_udp_payload(max_mtu);

    let subscriber = recorder::MtuUpdated::new();
    let events = subscriber.events();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(subscriber)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(10_000_000))?;
        Ok(addr)
    })
    .unwrap();

    let events_handle = events.lock().unwrap();
    events_handle.clone()
}

// if we specify jumbo frames on the endpoint and the network supports them,
// then jumbo frames should be negotiated.
#[test]
fn mtu_probe_jumbo_frame_test() {
    let events = mtu_updates(9_001);

    // handshake is padded to 1200, so we should immediate have an mtu of 1200
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

    // we binary search upwards 9001
    // this isn't the maximum mtu we'd find in practice, just the maximum mtu we
    // find with a payload of 10_000_000 bytes.
    let last_probe = events.last().unwrap();
    assert_eq!(last_probe.mtu, 8943);
}

// if we specify jumbo frames on the endpoint and the network does not support
// them, the connection should gracefully complete with a smaller mtu
#[test]
fn mtu_probe_jumbo_frame_unsupported_test() {
    let events = mtu_updates(1_500);
    let last_mtu = events.last().unwrap();
    // ETHERNET_MTU - UDP_HEADER_LEN - IPV4_HEADER_LEN
    assert_eq!(last_mtu.mtu, 1472);
}

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
            .with_event(subscriber)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(certificates::CERT_PEM)?
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

    // MTU remained jumbo despite the packet loss
    assert_eq!(8943, events.lock().unwrap().last().unwrap().mtu);
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
            .with_event(subscriber)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(certificates::CERT_PEM)?
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

    // MTU dropped to the minimum
    assert_eq!(1200, events.lock().unwrap().last().unwrap().mtu);
}

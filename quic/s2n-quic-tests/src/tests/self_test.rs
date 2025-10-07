// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A collection of tests to test the IO testing framework is working

use super::*;

/// Simple end-to-end test
#[test]
fn client_server_test() {
    test(Model::default(), client_server).unwrap();
}

/// Showing that the TxRecorder is working
#[test]
fn packet_sent_event_test() {
    let recorder = io::TxRecorder::default();
    let network_packets = recorder.get_packets();
    let subscriber = recorder::PacketSent::new();
    let events = subscriber.events();
    let mut server_socket = None;

    test((recorder, Model::default()), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(true), subscriber))?
            .start()?;
        let addr = start_server(server)?;
        // store addr in exterior scope so we can use it to filter packets
        // after the test ends
        server_socket = Some(addr);
        client(handle, addr)?;
        Ok(addr)
    })
    .unwrap();

    let server_socket = server_socket.unwrap();
    let mut events = events.lock().unwrap();
    let mut server_tx_network_packets: Vec<Packet> = network_packets
        .lock()
        .unwrap()
        .iter()
        .filter(|p| {
            let local_socket: SocketAddr = p.path.local_address.0.into();
            local_socket == server_socket
        })
        .cloned()
        .collect();

    // transmitted quic packets may be coalesced into a single datagram (network packet)
    // so it might be the case that network_packet[0] = quic_packet[0] + quic_packet[1]
    while let Some(server_packet) = server_tx_network_packets.pop() {
        let expected_len = server_packet.payload.len();

        let mut event_len = 0;
        while expected_len > event_len {
            event_len += events.pop().unwrap().packet_len;
        }

        assert_eq!(expected_len, event_len)
    }
    assert!(events.is_empty());
}

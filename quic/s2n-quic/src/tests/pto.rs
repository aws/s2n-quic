// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::encoder::scatter;
use s2n_quic_core::{
    event::api::{PacketHeader, Subject},
    packet::interceptor::{Interceptor, Packet},
};

/// This test ensures the PTO timer in the Handshake space is armed even
/// when the client does not otherwise receive or send any handshake
/// packets
#[test]
fn handshake_pto_timer_is_armed() {
    let model = Model::default();
    let pto_subscriber = recorder::Pto::new();
    let packet_sent_subscriber = recorder::PacketSent::new();
    let pto_events = pto_subscriber.events();
    let packet_sent_events = packet_sent_subscriber.events();

    test(model, |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_packet_interceptor(DropHandshakeTx)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            // We would expect this connection to time out since the server
            // is not able to send any handshake packets
            assert!(server.accept().await.is_none());
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event(((tracing_events(), pto_subscriber), packet_sent_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            // We would expect this connection to time out since the server
            // is not able to send any handshake packets
            assert!(client.connect(connect).await.is_err());
        });

        Ok(addr)
    })
    .unwrap();

    let pto_events = pto_events.lock().unwrap();
    let pto_count = *pto_events.iter().max().unwrap_or(&0) as usize;

    // Assert that the client sent some PTOs
    assert!(pto_count > 0);

    let packet_sent_events = packet_sent_events.lock().unwrap();
    let initial_packets_sent = packet_sent_events
        .iter()
        .filter(|&packet_sent| matches!(packet_sent.packet_header, PacketHeader::Initial { .. }))
        .count();
    let handshake_packets_sent = packet_sent_events
        .iter()
        .filter(|&packet_sent| matches!(packet_sent.packet_header, PacketHeader::Handshake { .. }))
        .count();

    // Assert that only 2 initial packets were sent (the Initial[ClientHello] and the Initial[ACK])
    assert_eq!(2, initial_packets_sent);

    // Assert that all handshake packets that were sent were due to the PTO timer firing.
    // The first PTO that fires will send a single packet, since there are no packets
    // in flight. Subsequent PTOs will send two packets.
    let expected_handshake_packet_count = pto_count * 2 - 1;
    assert_eq!(expected_handshake_packet_count, handshake_packets_sent);
}

/// Drops all outgoing handshake packets
struct DropHandshakeTx;

impl Interceptor for DropHandshakeTx {
    #[inline]
    fn intercept_tx_payload(
        &mut self,
        _subject: &Subject,
        packet: &Packet,
        payload: &mut scatter::Buffer,
    ) {
        if packet.number.space().is_handshake() {
            payload.clear();
        }
    }
}

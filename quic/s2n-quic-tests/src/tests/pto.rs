// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::encoder::scatter;
use s2n_quic::provider::limits::Limits;
use s2n_quic_core::{
    event::{
        api::{ConnectionMeta, PacketHeader, Subject},
        metrics::aggregate,
    },
    packet::interceptor::{Interceptor, Packet},
};
use std::net::ToSocketAddrs;

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

    test(model.clone(), |handle| {
        let metrics = if cfg!(windows) {
            aggregate::testing::Registry::no_snapshot()
        } else {
            aggregate::testing::Registry::snapshot()
        };

        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_packet_interceptor(DropHandshakeTx)?
            .with_event((
                tracing_events(true, model.clone()),
                metrics.subscriber("server"),
            ))?
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
            .with_event((
                (tracing_events(true, model.clone()), pto_subscriber),
                (packet_sent_subscriber, metrics.subscriber("client")),
            ))?
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

/// Test that configuring PTO jitter results in PTOs sent at different timestamps
#[test]
fn pto_jitter() {
    let model = Model::default();
    let pto_subscriber = recorder::Pto::new();
    let pto_subscriber_jitter = recorder::Pto::new();
    let datagram_sent_subscriber = DatagramSentTime::new();
    let datagram_sent_subscriber_jitter = DatagramSentTime::new();
    let pto_events = pto_subscriber.events();
    let pto_events_jitter = pto_subscriber_jitter.events();
    let datagram_sent_events = datagram_sent_subscriber.events();
    let datagram_sent_events_jitter = datagram_sent_subscriber_jitter.events();

    // Test 2 clients, one with jitter, one without
    // No server is needed, since the lack of acknowledgement from the server will
    // trigger PTO probes
    test(model.clone(), |handle| {
        let addr = "127.0.0.1:443".to_socket_addrs()?.next().unwrap();

        // Allow the handshake to go on for longer to allow for more PTO probes to be sent
        let limits = Limits::new().with_max_handshake_duration(Duration::from_secs(70))?;
        let client_no_jitter = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_limits(limits)?
            .with_event((
                (tracing_events(true, model.clone()), pto_subscriber),
                datagram_sent_subscriber,
            ))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            assert!(client_no_jitter.connect(connect).await.is_err());
        });

        // Configure 50% jitter
        let limits = Limits::new()
            .with_pto_jitter_percentage(50)?
            .with_max_handshake_duration(Duration::from_secs(70))?;
        let client_with_jitter = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_limits(limits)?
            .with_event((
                (tracing_events(true, model.clone()), pto_subscriber_jitter),
                datagram_sent_subscriber_jitter,
            ))?
            .with_random(Random::with_seed(123))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            assert!(client_with_jitter.connect(connect).await.is_err());
        });

        Ok(addr)
    })
    .unwrap();

    let pto_count = *pto_events.lock().unwrap().iter().max().unwrap_or(&0) as usize;
    let pto_count_jitter = *pto_events_jitter.lock().unwrap().iter().max().unwrap_or(&0) as usize;
    let datagram_sent_events = datagram_sent_events.lock().unwrap();
    let datagram_sent_events_jitter = datagram_sent_events_jitter.lock().unwrap();

    const EXPECTED_PTO_COUNT: usize = 6;

    // Assert that the clients sent some PTOs
    assert_eq!(pto_count, EXPECTED_PTO_COUNT);
    assert_eq!(pto_count_jitter, EXPECTED_PTO_COUNT);

    // Each client should send 1 initial datagram + 3 PTOs (each consisting of 2 datagrams)
    assert_eq!(datagram_sent_events.len(), 1 + EXPECTED_PTO_COUNT * 2);
    assert_eq!(
        datagram_sent_events_jitter.len(),
        1 + EXPECTED_PTO_COUNT * 2
    );

    let mut last_time = Duration::ZERO;
    let mut last_time_jittered = Duration::ZERO;
    for pto_count in 1..=EXPECTED_PTO_COUNT {
        let default_pto_base = Duration::from_millis(999);
        let expected_pto = default_pto_base * (2_u32.pow(pto_count as u32) - 1);
        let first_pto_datagram = datagram_sent_events[pto_count * 2 - 1];
        let second_pto_datagram = datagram_sent_events[pto_count * 2];
        let first_pto_datagram_jitter = datagram_sent_events_jitter[pto_count * 2 - 1];
        let second_pto_datagram_jitter = datagram_sent_events_jitter[pto_count * 2];
        let time_since_last = first_pto_datagram - last_time;
        let time_since_last_jittered = first_pto_datagram_jitter - last_time_jittered;
        last_time = first_pto_datagram;
        last_time_jittered = first_pto_datagram_jitter;

        // 2 PTO datagrams are sent at the same time
        assert_eq!(first_pto_datagram, second_pto_datagram);
        assert_eq!(first_pto_datagram_jitter, second_pto_datagram_jitter);

        // Without jitter the PTO is sent at the expected time
        assert_eq!(expected_pto, first_pto_datagram);

        // With jitter the PTO is sent at a different time
        assert_ne!(expected_pto, first_pto_datagram_jitter);

        // The jittered PTO should be within ±50% of the non jittered PTO
        let min_expected_jittered = (time_since_last * 50) / 100; // -50%
        let max_expected_jittered = (time_since_last * 150) / 100; // +50%

        assert!(time_since_last_jittered >= min_expected_jittered);
        assert!(time_since_last_jittered <= max_expected_jittered);
    }
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

#[derive(Clone, Default)]
pub struct DatagramSentTime {
    pub events: Arc<Mutex<Vec<Duration>>>,
}

impl DatagramSentTime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Arc<Mutex<Vec<Duration>>> {
        self.events.clone()
    }
}
impl events::Subscriber for DatagramSentTime {
    type ConnectionContext = DatagramSentTime;

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
        self.clone()
    }

    fn on_datagram_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &ConnectionMeta,
        _event: &events::DatagramSent,
    ) {
        context
            .events
            .lock()
            .unwrap()
            .push(meta.timestamp.duration_since_start());
    }
}

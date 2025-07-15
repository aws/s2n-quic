// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{
    client::Connect,
    connection::error,
    provider::{
        event::events::{self, Subscriber},
        io::testing::{primary, spawn, test, Model},
    },
    Client, Server,
};
use s2n_quic_core::{
    connection::Error,
    crypto::tls::testing::certificates,
    event::api::Subject,
    packet::{
        interceptor::{Ack, Interceptor},
        number::PacketNumberSpace,
    },
    stream::StreamError,
    varint::VarInt,
};
use s2n_quic_tests::*;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn optimistic_ack_mitigation() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));
    const LEN: usize = 1_000_000;

    let server_subscriber = recorder::PacketSkipped::new();
    let server_events = server_subscriber.events();
    let client_subscriber = recorder::PacketSkipped::new();
    let client_events = server_subscriber.events();
    test(model, |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), server_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), client_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let mut stream = conn.accept_bidirectional_stream().await.unwrap().unwrap();

            let mut recv_len = 0;
            while let Some(chunk) = stream.receive().await.unwrap() {
                recv_len += chunk.len();
            }
            assert_eq!(LEN, recv_len);
        });

        Ok(addr)
    })
    .unwrap();

    let server_skip_count = server_events
        .lock()
        .unwrap()
        .iter()
        .filter(|reason| {
            matches!(
                reason,
                events::PacketSkipReason::OptimisticAckMitigation { .. }
            )
        })
        .count();
    let client_skip_count = client_events
        .lock()
        .unwrap()
        .iter()
        .filter(|reason| {
            matches!(
                reason,
                events::PacketSkipReason::OptimisticAckMitigation { .. }
            )
        })
        .count();

    // Verify that both client and server are skipping packets for Optimistic
    // Ack attack mitigation.
    //
    // The exact number of skipped packets depends on randomness, so this test may be changed by
    // unrelated changes. The important thing is that both numbers are non-zero.
    assert_eq!(server_skip_count, 5);
    assert_eq!(client_skip_count, 5);
}

// Mimic an Optimistic Ack attack and confirm the connection is closed with
// the appropriate error.
//
// Use the SkipSubscriber to record the skipped packet_number and then use
// the SkipInterceptor to inject an ACK for that packet.
#[test]
fn detect_optimistic_ack() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));
    const LEN: usize = 1_000_000;

    let skip_pn = Arc::new(Mutex::new(None));
    let skip_subscriber = SkipSubscriber {
        skip_packet_number: skip_pn.clone(),
    };
    let skip_interceptor = SkipInterceptor {
        skip_packet_number: skip_pn,
    };
    test(model, |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), skip_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_packet_interceptor(skip_interceptor)?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            let send_result = stream.flush().await;
            // connection should abort since we inject a skip packet number
            match send_result.err() {
                Some(StreamError::ConnectionError {
                    error: Error::Transport { code, reason, .. },
                    ..
                }) => {
                    assert_eq!(code, error::Code::PROTOCOL_VIOLATION);
                    assert_eq!(reason, "received an ACK for a packet that was not sent")
                }
                result => unreachable!("Unexpected result: {:?}", result),
            }
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let mut stream = conn.accept_bidirectional_stream().await.unwrap().unwrap();

            let mut recv_len = 0;

            while let Ok(Some(chunk)) = stream.receive().await {
                recv_len += chunk.len();
            }
            // connection aborts before completing the transfer
            assert_ne!(LEN, recv_len);
        });

        Ok(addr)
    })
    .unwrap();
}

struct SkipSubscriber {
    skip_packet_number: Arc<Mutex<Option<u64>>>,
}

impl Subscriber for SkipSubscriber {
    type ConnectionContext = Arc<Mutex<Option<u64>>>;

    fn create_connection_context(
        &mut self,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        _info: &s2n_quic_core::event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
        self.skip_packet_number.clone()
    }

    fn on_packet_skipped(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        event: &s2n_quic_core::event::api::PacketSkipped,
    ) {
        *context.lock().unwrap() = Some(event.number);
    }
}

struct SkipInterceptor {
    skip_packet_number: Arc<Mutex<Option<u64>>>,
}

impl Interceptor for SkipInterceptor {
    fn intercept_rx_ack<A: Ack>(&mut self, _subject: &Subject, ack: &mut A) {
        if !matches!(ack.space(), PacketNumberSpace::ApplicationData) {
            return;
        }
        let skip_packet_number = self.skip_packet_number.lock().unwrap().take();
        if let Some(skip_packet_number) = skip_packet_number {
            let skip_packet_number = VarInt::new(skip_packet_number).unwrap();
            ack.insert_range(skip_packet_number..=skip_packet_number);
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::encoder::scatter;
use s2n_quic::connection::Error;
use s2n_quic_core::{
    event::api::Subject,
    packet::interceptor::{Interceptor, Packet},
    transport,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn empty_new_token_closes_client_with_frame_encoding_error() {
    let injected = Arc::new(AtomicBool::new(false));
    let closed = recorder::ConnectionClosed::new();
    let errors = closed.events();

    test(Model::default(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_packet_interceptor(EmptyNewToken(injected.clone()))?
            .start()?;
        let addr = start_server(server)?;
        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(closed)?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            // The invalid packet can arrive during connect or immediately after it.
            if let Ok(mut connection) = client.connect(connect).await {
                connection.accept_bidirectional_stream().await.unwrap_err();
            }
        });
        Ok(())
    })
    .unwrap();

    assert!(injected.load(Ordering::Relaxed));
    let errors = errors.lock().unwrap();
    assert_eq!(errors.len(), 1);
    let Error::Transport { code, .. } = errors[0] else {
        panic!("expected a transport error, got {:?}", errors[0]);
    };
    assert_eq!(code, transport::Error::FRAME_ENCODING_ERROR.code);
}

struct EmptyNewToken(Arc<AtomicBool>);

impl Interceptor for EmptyNewToken {
    fn intercept_tx_payload(
        &mut self,
        _subject: &Subject,
        packet: &Packet,
        payload: &mut scatter::Buffer,
    ) {
        if packet.number.space().is_initial() || packet.number.space().is_handshake() {
            return;
        }
        if self.0.load(Ordering::Relaxed) {
            return;
        }
        let payload = payload.flatten().as_mut_slice();
        if payload.len() < 2 {
            return;
        }
        // Replace the plaintext before packet protection: NEW_TOKEN, length 0,
        // followed by PADDING. Keep the packet length unchanged.
        payload.fill(0);
        payload[0] = 0x07;
        self.0.store(true, Ordering::Relaxed);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic_core::{
    dc::testing::MockDcEndpoint,
    event::Subscriber,
    stateless_reset::{
        token::testing::{TEST_TOKEN_1, TEST_TOKEN_2},
        Token,
    },
};

const SERVER_TOKENS: [Token; 1] = [TEST_TOKEN_1];
const CLIENT_TOKENS: [Token; 1] = [TEST_TOKEN_2];
const MTU: u16 = 1200;

#[derive(Default)]
struct ConnectionClosedSubscriber {
    connection_closed: bool,
    datagram_count: u8,
}

impl Subscriber for ConnectionClosedSubscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        _info: &s2n_quic_core::event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_connection_closed(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        _event: &s2n_quic_core::event::api::ConnectionClosed,
    ) {
        self.connection_closed = true;
    }

    fn on_datagram_received(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        _event: &s2n_quic_core::event::api::DatagramReceived,
    ) {
        self.datagram_count += 1;
    }
    fn on_frame_received(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &s2n_quic_core::event::api::ConnectionMeta,
        event: &s2n_quic_core::event::api::FrameReceived,
    ) {
        // No frames are processed past connection closure
        assert!(!self.connection_closed);

        // These assertions exist to check that we have set up the test scenario correctly.
        if self.datagram_count == 3 {
            // First packet in third datagram is Handshake
            assert!(matches!(
                event.packet_header,
                s2n_quic_core::event::api::PacketHeader::Handshake { .. }
            ));
        } else {
            // No Handshake packets before the third datagram carry Crypto
            if matches!(
                event.packet_header,
                s2n_quic_core::event::api::PacketHeader::Handshake { .. }
            ) {
                assert!(!matches!(
                    event.frame,
                    s2n_quic_core::event::api::Frame::Crypto { .. }
                ));
            }
        }
    }
}

/// This test recreates a niche bug seen on a dc-quic connection as follows:
/// 1. The connection's MTU is low enough that the server's certificate is fragmented across two datagrams
///    and can't fit into the server's first datagram.
/// 2. The client responds with two datagrams; its Handshake packet and OneRtt packet get coalesced
///    into the second datagram.
/// 3. The server closes the connection when given the client's certificate in the on_path_secrets application
///    data callback.
/// 4. The server then continues to process the remaining OneRTT packet in the datagram, due to a bug in
///    the error codepath. This causes a panic in the on_peer_stateless_reset callback as it expects
///    the path_secret to be created already.
///
/// The test asserts that no packets are processed after connection closure.
#[test]
fn handle_packet_failure() {
    let model = Model::default();
    model.set_max_udp_payload(MTU);

    test(model.clone(), |handle| {
        let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS).with_failing_path_secrets())?
            .with_event((
                tracing_events(true, model.clone()),
                ConnectionClosedSubscriber::default(),
            ))?
            .start()?;

        let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(
                handle
                    .builder()
                    .with_internal_recv_buffer_size(MTU as usize)?
                    .build()?,
            )?
            .with_tls(client_tls)?
            .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;

        let addr = server.local_addr()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            assert!(matches!(conn.accept_bidirectional_stream().await, Ok(None)));
        });

        spawn(async move {
            if let Some(_) = server.accept().await {
                panic!("connection should not be accepted on path_secrets failure");
            }
        });

        Ok(addr)
    })
    .unwrap();
}

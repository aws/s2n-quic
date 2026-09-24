// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::DecoderBufferMut;
use s2n_quic::provider::{
    connection_id,
    endpoint_limits::{ConnectionAttempt, Limiter, Outcome},
    event::events::{EndpointDatagramDropped, EndpointMeta, EndpointPacketSent},
};
use s2n_quic_core::{
    connection,
    event::api::{DatagramDropReason, PacketHeader, Subject},
    packet::interceptor::{Datagram, Interceptor},
};

/// Retries every connection attempt, to exercise the Retry path.
struct AlwaysRetry;

impl Limiter for AlwaysRetry {
    fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
        Outcome::retry()
    }
}

/// Overwrites the first datagram the server receives with an Initial packet whose
/// Destination Connection ID is shorter than `InitialId::MIN_LEN`.
///
/// The length is chosen to be at least `LocalId::MIN_LEN` so the datagram survives the
/// endpoint's routing lookup and reaches the retry dispatch.
#[derive(Default)]
struct ShortDcidInitial {
    injected: bool,
}

impl Interceptor for ShortDcidInitial {
    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        let payload = payload.into_less_safe_slice();

        if self.injected {
            return DecoderBufferMut::new(payload);
        }
        self.injected = true;

        let mut packet = Vec::new();
        // long header, fixed bit, Initial packet type
        packet.push(0xc0);
        // version 1
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        // Destination Connection ID, 4 bytes: >= LocalId::MIN_LEN but < InitialId::MIN_LEN
        packet.push(0x04);
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        // zero length Source Connection ID
        packet.push(0x00);
        // zero length token, which selects the retry path
        packet.push(0x00);
        // payload length of 0, encoded as a 2 byte varint
        packet.extend_from_slice(&[0x40, 0x00]);

        assert!(
            packet.len() <= payload.len(),
            "the crafted packet must fit in the datagram being replaced"
        );

        payload[..packet.len()].copy_from_slice(&packet);
        payload[packet.len()..].fill(0);

        DecoderBufferMut::new(payload)
    }
}

/// Records the destination connection ID length of each inbound Initial packet.
#[derive(Clone, Default)]
struct InitialDcidLens(Arc<Mutex<Vec<usize>>>);

impl InitialDcidLens {
    fn get(&self) -> Vec<usize> {
        self.0.lock().unwrap().clone()
    }
}

impl Interceptor for InitialDcidLens {
    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        let payload = payload.into_less_safe_slice();

        // long header with the Initial packet type, then the version, then the destination
        // connection ID length
        if payload.first().is_some_and(|tag| tag >> 4 == 0b1100) {
            if let Some(len) = payload.get(5) {
                self.0.lock().unwrap().push(*len as usize);
            }
        }

        DecoderBufferMut::new(payload)
    }
}

#[derive(Clone, Default)]
struct EndpointEvents {
    drops: Arc<Mutex<Vec<EndpointDatagramDropped>>>,
    retries_sent: Arc<Mutex<usize>>,
}

impl EndpointEvents {
    fn dropped_invalid_dcid(&self) -> bool {
        self.drops.lock().unwrap().iter().any(|event| {
            matches!(
                event.reason,
                DatagramDropReason::InvalidDestinationConnectionId { .. }
            )
        })
    }

    fn retries_sent(&self) -> usize {
        *self.retries_sent.lock().unwrap()
    }
}

impl events::Subscriber for EndpointEvents {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_endpoint_datagram_dropped(
        &mut self,
        _meta: &EndpointMeta,
        event: &EndpointDatagramDropped,
    ) {
        self.drops.lock().unwrap().push(event.clone());
    }

    fn on_endpoint_packet_sent(&mut self, _meta: &EndpointMeta, event: &EndpointPacketSent) {
        if matches!(event.packet_header, PacketHeader::Retry { .. }) {
            *self.retries_sent.lock().unwrap() += 1;
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
//= type=test
//# When an Initial packet is sent by a client that has not previously
//# received an Initial or Retry packet from the server, the client
//# populates the Destination Connection ID field with an unpredictable
//# value.  This Destination Connection ID MUST be at least 8 bytes in
//# length.
//
//= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
//= type=test
//# Servers MUST drop incoming packets under all other circumstances.
//
// An Initial packet whose destination connection ID is shorter than InitialId::MIN_LEN is
// dropped rather than answered with a Retry, and the endpoint continues serving connections.
#[test]
fn retry_with_short_destination_connection_id() {
    let model = Model::default();
    let events = EndpointEvents::default();
    let observed = events.clone();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(false, model.clone()), events))?
            .with_random(Random::with_seed(456))?
            .with_endpoint_limits(AlwaysRetry)?
            .with_packet_interceptor(ShortDcidInitial::default())?
            .start()?;

        let server_addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(789))?
            .start()?;

        primary::spawn(async move {
            // The malformed datagram replaced the first Initial, so this only succeeds if the
            // endpoint survived it and went on to serve the retransmission.
            let connect = Connect::new(server_addr).with_server_name("localhost");
            client.connect(connect).await.unwrap();
        });

        Ok(())
    })
    .unwrap();

    assert!(
        observed.dropped_invalid_dcid(),
        "the short destination connection id should be reported as invalid"
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
//= type=test
//# When an Initial packet is sent by a client that has not previously
//# received an Initial or Retry packet from the server, the client
//# populates the Destination Connection ID field with an unpredictable
//# value.  This Destination Connection ID MUST be at least 8 bytes in
//# length.
//
// The 8 byte minimum applies only to the connection ID a client invents before hearing from the
// server. An Initial sent in response to a Retry carries the server's LocalId, which may be as
// short as LocalId::MIN_LEN, so the server must still accept it.
#[test]
fn retry_response_may_use_short_destination_connection_id() {
    let model = Model::default();
    let events = EndpointEvents::default();
    let observed = events.clone();
    let dcid_lens = InitialDcidLens::default();
    let observed_dcid_lens = dcid_lens.clone();

    test(model.clone(), |handle| {
        let server_ids = connection_id::default::Format::builder()
            .with_len(connection::LocalId::MIN_LEN)
            .unwrap()
            .build()
            .unwrap();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(false, model.clone()), events))?
            .with_random(Random::with_seed(456))?
            .with_connection_id(server_ids)?
            .with_endpoint_limits(AlwaysRetry)?
            .with_packet_interceptor(dcid_lens)?
            .start()?;

        let server_addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(789))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            client.connect(connect).await.unwrap();
        });

        Ok(())
    })
    .unwrap();

    assert_eq!(observed.retries_sent(), 1);

    // The client's first Initial uses its own full length connection ID, and the one it sends
    // after the Retry uses the server's shorter one.
    assert_eq!(
        observed_dcid_lens.get(),
        [connection::InitialId::MIN_LEN, connection::LocalId::MIN_LEN]
    );
    assert!(!observed.dropped_invalid_dcid());
}

// Pins the length window the tests above rely on.
#[test]
fn short_destination_connection_id_bypasses_routing_lookup() {
    for len in connection::LocalId::MIN_LEN..connection::InitialId::MIN_LEN {
        let bytes = vec![0u8; len];
        assert!(connection::LocalId::try_from_bytes(&bytes).is_some());
        assert!(connection::InitialId::try_from_bytes(&bytes).is_none());
    }
}

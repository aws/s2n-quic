// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::encoder::scatter;
use s2n_quic::provider::{
    endpoint_limits::{ConnectionAttempt, Limiter, Outcome},
    event::events::{
        EndpointDatagramDropped, EndpointDatagramSent, EndpointMeta, EndpointPacketSent,
    },
};
use s2n_quic_core::{
    event::api::{DatagramDropReason, PacketHeader, Subject},
    packet::interceptor::{Interceptor, Packet},
    path::MINIMUM_MAX_DATAGRAM_SIZE,
};

/// Requests address validation for every connection attempt, which queues a Retry packet.
struct AlwaysRetry;

impl Limiter for AlwaysRetry {
    fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
        Outcome::retry()
    }
}

/// Refuses every connection attempt, which queues an Initial packet containing CONNECTION_CLOSE.
struct AlwaysClose;

impl Limiter for AlwaysClose {
    fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
        Outcome::close()
    }
}

/// Strips the PADDING frames from every Initial packet the client sends, so each datagram
/// carrying an Initial is smaller than `MINIMUM_MAX_DATAGRAM_SIZE`.
///
/// Padding is written before interception and the packet length is recomputed afterwards, so the
/// resulting datagram is a well formed, correctly encrypted Initial that is simply too small.
/// That is exactly what an off-path attacker can send: the Initial packet protection keys are
/// derived from the destination connection ID, so no secret is needed to produce one.
struct StripInitialPadding;

impl Interceptor for StripInitialPadding {
    fn intercept_tx_payload(
        &mut self,
        _subject: &Subject,
        packet: &Packet,
        payload: &mut scatter::Buffer,
    ) {
        if !packet.number.space().is_initial() {
            return;
        }

        let buffer = payload.flatten();
        let written = buffer.as_mut_slice();
        let written_len = written.len();

        // PADDING frames are encoded as zero bytes, so trim the zero suffix. A floor is applied
        // so the remaining payload is always long enough for header protection sampling.
        let end = written
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(0, |idx| idx + 1)
            .max(32);

        if end < written_len {
            buffer.set_position(end);
        }
    }
}

/// Records the responses an endpoint generates outside of a connection.
///
/// `on_endpoint_packet_sent` and `on_endpoint_datagram_sent` are only published by the endpoint's
/// dispatch queues, so counting them captures every byte the endpoint sends in response to
/// a datagram that has not been attributed to a connection.
#[derive(Clone, Default)]
struct EndpointEvents {
    drops: Arc<Mutex<Vec<EndpointDatagramDropped>>>,
    datagrams_sent: Arc<Mutex<Vec<EndpointDatagramSent>>>,
    packets_sent: Arc<Mutex<Vec<&'static str>>>,
}

impl EndpointEvents {
    /// The lengths of the datagrams dropped for being an undersized Initial.
    fn undersized_drop_lens(&self) -> Vec<u16> {
        self.drops
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                matches!(
                    event.reason,
                    DatagramDropReason::UndersizedInitialPacket { .. }
                )
            })
            .map(|event| event.len)
            .collect()
    }

    fn datagrams_sent(&self) -> usize {
        self.datagrams_sent.lock().unwrap().len()
    }

    fn bytes_sent(&self) -> usize {
        self.datagrams_sent
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.len as usize)
            .sum()
    }

    fn packets_sent(&self) -> Vec<&'static str> {
        self.packets_sent.lock().unwrap().clone()
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

    fn on_endpoint_datagram_sent(&mut self, _meta: &EndpointMeta, event: &EndpointDatagramSent) {
        self.datagrams_sent.lock().unwrap().push(event.clone());
    }

    fn on_endpoint_packet_sent(&mut self, _meta: &EndpointMeta, event: &EndpointPacketSent) {
        let kind = match event.packet_header {
            PacketHeader::Retry { .. } => "retry",
            PacketHeader::Initial { .. } => "initial",
            PacketHeader::VersionNegotiation { .. } => "version_negotiation",
            _ => "other",
        };
        self.packets_sent.lock().unwrap().push(kind);
    }
}

/// Builds a client that strips the padding from its Initial packets and gives up on the handshake
/// quickly, since a server that correctly discards those packets never answers them.
fn undersized_initial_client(handle: &io::Handle, model: Model) -> Result<Client> {
    let limits = provider::limits::Limits::new()
        .with_max_handshake_duration(Duration::from_secs(1))
        .expect("handshake duration is valid");

    Ok(Client::builder()
        .with_io(handle.builder().build()?)?
        .with_tls(certificates::CERT_PEM)?
        .with_event(tracing_events(true, model))?
        .with_limits(limits)?
        .with_random(Random::with_seed(789))?
        .with_packet_interceptor(StripInitialPadding)?
        .start()?)
}

/// Drives a handshake in which every Initial the client sends is undersized, and asserts that the
/// server discarded all of them without sending a single byte in response.
fn undersized_initial_test<L: 'static + Limiter>(limiter: L) {
    let model = Model::default();
    let events = EndpointEvents::default();
    let observed = events.clone();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(false, model.clone()), events))?
            .with_random(Random::with_seed(456))?
            .with_endpoint_limits(limiter)?
            .start()?;

        let server_addr = start_server(server)?;
        let client = undersized_initial_client(handle, model.clone())?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            // The handshake cannot complete: every Initial the client sends is undersized and
            // must be discarded. All that matters here is that the server sends nothing back.
            assert!(client.connect(connect).await.is_err());
        });

        Ok(())
    })
    .unwrap();

    // An undersized Initial is not a request for anything, so the
    // endpoint must not reflect any bytes toward its unvalidated source address.
    assert_eq!(observed.packets_sent(), Vec::<&str>::new());
    assert_eq!(observed.datagrams_sent(), 0);
    assert_eq!(observed.bytes_sent(), 0);

    // Assert the dropped packets are less than 1,200 bytes
    let drop_lens = observed.undersized_drop_lens();
    assert!(!drop_lens.is_empty(),);
    for len in drop_lens {
        assert!((1..MINIMUM_MAX_DATAGRAM_SIZE).contains(&len));
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
//= type=test
//# A server MUST discard an Initial packet that is carried in a UDP
//# datagram with a payload that is smaller than the smallest allowed
//# maximum datagram size of 1200 bytes.

//
// An Initial packet in an undersized datagram is discarded before the endpoint limiter runs, so a
// limiter asking for address validation cannot be ran. No Retry is sent
// to the unvalidated and potentially spoofed source address.
#[test]
fn undersized_initial_elicits_no_retry() {
    undersized_initial_test(AlwaysRetry);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.1
//= type=test
//# A server MUST discard an Initial packet that is carried in a UDP
//# datagram with a payload that is smaller than the smallest allowed
//# maximum datagram size of 1200 bytes.

// The same holds for a limiter that refuses the connection. The Initial packet carrying
// CONNECTION_CLOSE is a response, and responses to undersized Initial packets are not allowed.
#[test]
fn undersized_initial_elicits_no_connection_close() {
    undersized_initial_test(AlwaysClose);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.2
//= type=test
//# Upon receiving the client's Initial packet, the server can request
//# address validation by sending a Retry packet (Section 17.2.5)
//# containing a token.

// This is a positive test. The new check applies only to undersized datagrams.
// A conforming client that receives retry can still complete the handshake with
// a initial packet that fulfills the size requirements.
#[test]
fn conforming_initial_still_elicits_retry() {
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

    assert_eq!(observed.packets_sent(), ["retry"]);
    assert!(observed.undersized_drop_lens().is_empty(),);
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stateless liveness protocol for the dcQUIC handshake endpoint.
//!
//! A request and response are each exactly 32 bytes:
//! `tag[1]`, `magic[7]`, `version[1]`, `kind[1]`, zeroed reserved bytes `[6]`, and a random
//! nonce `[16]`.
//! Unknown versions, kinds, nonzero reserved bytes, and packets of any other length are ignored
//! without a response.
//!
//! The protocol is intentionally unauthenticated because it runs before the handshake. The random
//! nonce prevents off-path responses from being accepted, but an on-path peer can forge liveness.
//! Equal request and response sizes prevent reflection amplification. Servers should be deployed
//! before clients enable probing; servers without this protocol appear unresponsive until the probe
//! window expires, after which the normal handshake still proceeds.

use crate::packet::tag::HANDSHAKE_PROBE_TAG;
use s2n_quic::provider::io::Provider as IoProvider;
use s2n_quic_core::{
    endpoint::{CloseError, Endpoint as QuicEndpoint},
    inet::{datagram, SocketAddress},
    io::{rx, tx},
    path::{self, mtu, Handle as _},
    time::{Clock, Timestamp},
};
use std::{
    collections::VecDeque,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    task::{Context, Poll},
    time::Duration,
};
use tokio::net::UdpSocket;

/// Size of a handshake liveness request.
pub const REQUEST_SIZE: usize = 32;
/// Size of a handshake liveness response.
pub const RESPONSE_SIZE: usize = 32;
/// Receive buffer size used to distinguish an exact packet from a truncated oversized datagram.
pub const RECEIVE_BUFFER_SIZE: usize = RESPONSE_SIZE + 1;
// Wire format: dcQUIC probe tag (1), magic (7), version (1), kind (1), reserved (6), nonce (16).
// The tag is reserved from other dcQUIC packet types, and the full magic distinguishes a probe
// before handing other datagrams to the QUIC endpoint.
const MAGIC: [u8; 8] = [
    HANDSHAKE_PROBE_TAG,
    b'D',
    b'C',
    b'Q',
    b'P',
    b'R',
    b'O',
    b'B',
];
const VERSION: u8 = 1;
const REQUEST_KIND: u8 = 1;
const RESPONSE_KIND: u8 = 2;
const RESERVED: core::ops::Range<usize> = 10..16;
const NONCE: core::ops::Range<usize> = 16..RESPONSE_SIZE;
// Probe responses are best-effort. Bound queued work under a request burst and drop excess
// requests rather than allowing unauthenticated traffic to allocate without limit.
const MAX_PENDING_RESPONSES: usize = 64;
const _: () = assert!(RESPONSE_SIZE == REQUEST_SIZE);

/// A stateless dcQUIC handshake liveness request.
pub struct ProbeRequest {
    packet: [u8; REQUEST_SIZE],
}

impl ProbeRequest {
    /// Creates a request with a random nonce.
    pub fn new() -> Self {
        let mut nonce = [0; 16];
        #[expect(clippy::unwrap_used, reason = "no recovery from broken entropy pool")]
        aws_lc_rs::rand::fill(&mut nonce).unwrap();
        Self {
            packet: encode(REQUEST_KIND, &nonce),
        }
    }

    /// Returns the encoded request datagram.
    pub fn as_bytes(&self) -> &[u8; REQUEST_SIZE] {
        &self.packet
    }

    /// Returns whether `packet` is the response correlated to this request.
    pub fn is_response(&self, packet: &[u8]) -> bool {
        decode(packet, RESPONSE_KIND, RESPONSE_SIZE)
            .is_some_and(|nonce| nonce[..] == self.packet[NONCE])
    }

    /// Decodes an exact, well-formed request datagram.
    pub fn from_bytes(packet: &[u8]) -> Option<Self> {
        let nonce = *decode(packet, REQUEST_KIND, REQUEST_SIZE)?;
        Some(Self {
            packet: encode(REQUEST_KIND, &nonce),
        })
    }

    /// Creates the response correlated to this request.
    pub fn response(&self) -> ProbeResponse {
        let mut nonce = [0; 16];
        nonce.copy_from_slice(&self.packet[NONCE]);
        ProbeResponse {
            packet: encode(RESPONSE_KIND, &nonce),
        }
    }
}

impl Default for ProbeRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// A stateless dcQUIC handshake liveness response.
#[derive(Clone, Copy)]
pub struct ProbeResponse {
    packet: [u8; RESPONSE_SIZE],
}

impl ProbeResponse {
    /// Returns the encoded response datagram.
    pub fn as_bytes(&self) -> &[u8; RESPONSE_SIZE] {
        &self.packet
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Result {
    Responsive,
    Unresponsive,
}

/// Checks whether a dcQUIC handshake endpoint responds without starting a handshake.
///
/// The request carries a random nonce. A dcQUIC handshake server consumes the request before QUIC
/// packet processing and sends a same-sized, nonce-correlated response from its handshake port.
pub(super) async fn probe(peer: SocketAddr, timeout: Duration) -> io::Result<Result> {
    let local_addr = match peer {
        SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let socket = UdpSocket::bind(local_addr).await?;
    socket.connect(peer).await?;

    let request = ProbeRequest::new();
    match socket.send(request.as_bytes()).await {
        Ok(_) => {}
        Err(error) if is_unresponsive(&error) => return Ok(Result::Unresponsive),
        Err(error) => return Err(error),
    }

    let deadline = tokio::time::Instant::now() + timeout;
    let mut response = [0u8; RECEIVE_BUFFER_SIZE];
    loop {
        match tokio::time::timeout_at(deadline, socket.recv(&mut response)).await {
            Ok(Ok(len)) if request.is_response(&response[..len]) => {
                return Ok(Result::Responsive);
            }
            // A connected UDP socket can still contain a delayed or unrelated datagram. Ignore
            // it while preserving the original probe deadline.
            Ok(Ok(_)) => continue,
            Ok(Err(error)) if is_unresponsive(&error) => return Ok(Result::Unresponsive),
            Ok(Err(error)) => return Err(error),
            Err(_) => return Ok(Result::Unresponsive),
        }
    }
}

fn encode<const LEN: usize>(kind: u8, nonce: &[u8; 16]) -> [u8; LEN] {
    let mut packet = [0u8; LEN];
    packet[..MAGIC.len()].copy_from_slice(&MAGIC);
    packet[MAGIC.len()] = VERSION;
    packet[MAGIC.len() + 1] = kind;
    packet[NONCE].copy_from_slice(nonce);
    packet
}

fn decode(packet: &[u8], kind: u8, expected_len: usize) -> Option<&[u8; 16]> {
    if packet.len() != expected_len
        || packet[..MAGIC.len()] != MAGIC
        || packet[MAGIC.len()] != VERSION
        || packet[MAGIC.len() + 1] != kind
        || packet[RESERVED].iter().any(|byte| *byte != 0)
    {
        return None;
    }

    packet[NONCE].try_into().ok()
}

fn is_unresponsive(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::TimedOut
            | io::ErrorKind::WouldBlock
    )
}

/// Wraps an s2n-quic I/O provider with stateless probe handling.
pub(super) struct Provider<P>(P);

impl<P> Provider<P> {
    pub(super) fn new(inner: P) -> Self {
        Self(inner)
    }
}

impl<P: IoProvider> IoProvider for Provider<P> {
    type PathHandle = P::PathHandle;
    type Error = P::Error;

    fn start<E: QuicEndpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> core::result::Result<SocketAddress, Self::Error> {
        self.0.start(Endpoint {
            inner: endpoint,
            responses: VecDeque::new(),
            probe_wakeup: false,
        })
    }
}

struct Endpoint<E: QuicEndpoint> {
    inner: E,
    responses: VecDeque<(E::PathHandle, ProbeResponse)>,
    probe_wakeup: bool,
}

impl<E: QuicEndpoint> QuicEndpoint for Endpoint<E> {
    type PathHandle = E::PathHandle;
    type Subscriber = E::Subscriber;

    const ENDPOINT_TYPE: s2n_quic_core::endpoint::Type = E::ENDPOINT_TYPE;

    fn receive<Rx, C>(&mut self, queue: &mut Rx, clock: &C)
    where
        Rx: rx::Queue<Handle = Self::PathHandle>,
        C: Clock,
    {
        self.inner.receive(
            &mut ReceiveQueue {
                inner: queue,
                responses: &mut self.responses,
            },
            clock,
        );
    }

    fn transmit<Tx, C>(&mut self, queue: &mut Tx, clock: &C)
    where
        Tx: tx::Queue<Handle = Self::PathHandle>,
        C: Clock,
    {
        // Reserve at most one slot per event-loop pass. This guarantees progress for a liveness
        // response under sustained QUIC output without allowing probe traffic to starve QUIC.
        let mut pushed = false;
        if queue.has_capacity() {
            if let Some((path, response)) = self.responses.pop_front() {
                match queue.push((path, response.packet)) {
                    Ok(_) => pushed = true,
                    Err(tx::Error::AtCapacity) => {
                        self.responses.push_front((path, response));
                    }
                    Err(_) => {}
                }
            }
        }
        if pushed {
            queue.flush();
        }

        self.inner.transmit(queue, clock);
        self.probe_wakeup = !self.responses.is_empty() && queue.has_capacity();
    }

    fn poll_wakeups<C: Clock>(
        &mut self,
        cx: &mut Context<'_>,
        clock: &C,
    ) -> Poll<core::result::Result<usize, CloseError>> {
        match self.inner.poll_wakeups(cx, clock) {
            Poll::Pending if core::mem::take(&mut self.probe_wakeup) => Poll::Ready(Ok(1)),
            result => result,
        }
    }

    fn timeout(&self) -> Option<Timestamp> {
        self.inner.timeout()
    }

    fn set_mtu_config(&mut self, mtu_config: mtu::Config) {
        self.inner.set_mtu_config(mtu_config);
    }

    fn subscriber(&mut self) -> &mut Self::Subscriber {
        self.inner.subscriber()
    }
}

struct ReceiveQueue<'a, Q: rx::Queue> {
    inner: &'a mut Q,
    responses: &'a mut VecDeque<(Q::Handle, ProbeResponse)>,
}

impl<Q: rx::Queue> rx::Queue for ReceiveQueue<'_, Q> {
    type Handle = Q::Handle;

    fn for_each<F: FnMut(datagram::Header<Self::Handle>, &mut [u8])>(&mut self, mut on_packet: F) {
        self.inner.for_each(|header, payload| {
            if let Some(request) = ProbeRequest::from_bytes(payload) {
                if self.responses.len() < MAX_PENDING_RESPONSES
                    && !path::remote_port_blocked(header.path.remote_address().port())
                {
                    self.responses.push_back((header.path, request.response()));
                }
            } else {
                on_packet(header, payload);
            }
        });
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_have_stable_wire_format() {
        let nonce = *b"0123456789abcdef";
        let request: [u8; REQUEST_SIZE] = encode(REQUEST_KIND, &nonce);
        let response: [u8; RESPONSE_SIZE] = encode(RESPONSE_KIND, &nonce);

        assert_eq!(
            request,
            *b"\x68DCQPROB\x01\x01\0\0\0\0\0\x000123456789abcdef"
        );
        assert_eq!(
            response,
            *b"\x68DCQPROB\x01\x02\0\0\0\0\0\x000123456789abcdef"
        );
        assert_eq!(response.len(), request.len());
    }

    #[test]
    fn probe_tag_is_reserved_from_other_dc_packet_types() {
        let probe = ProbeRequest::new();
        assert_eq!(probe.as_bytes()[0], HANDSHAKE_PROBE_TAG);
        assert!(s2n_codec::DecoderBuffer::new(probe.as_bytes())
            .decode::<crate::packet::Tag>()
            .is_err());
    }

    #[test]
    fn request_and_response_are_correlated() {
        let request = ProbeRequest::new();
        let response = request.response();

        assert_eq!(request.as_bytes().len(), REQUEST_SIZE);
        assert_eq!(response.as_bytes().len(), RESPONSE_SIZE);
        assert!(request.is_response(response.as_bytes()));
        assert!(!ProbeRequest::new().is_response(response.as_bytes()));
        assert!(ProbeRequest::from_bytes(request.as_bytes()).is_some());
    }

    #[test]
    fn rejects_malformed_packets() {
        let request = ProbeRequest::new();

        for index in 0..16 {
            let mut malformed = *request.as_bytes();
            malformed[index] ^= 1;
            assert!(
                ProbeRequest::from_bytes(&malformed).is_none(),
                "index {index}"
            );
        }

        assert!(ProbeRequest::from_bytes(&request.as_bytes()[..REQUEST_SIZE - 1]).is_none());
        assert!(ProbeRequest::from_bytes(&[0; REQUEST_SIZE + 1]).is_none());
        assert!(ProbeRequest::from_bytes(request.response().as_bytes()).is_none());
    }

    #[tokio::test]
    async fn receives_correlated_response_after_unrelated_datagram() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();

        let responder = tokio::spawn(async move {
            let mut packet = [0u8; REQUEST_SIZE];
            let (len, peer) = server.recv_from(&mut packet).await.unwrap();
            let request = ProbeRequest::from_bytes(&packet[..len]).unwrap();

            server.send_to(b"unrelated", peer).await.unwrap();
            server
                .send_to(request.response().as_bytes(), peer)
                .await
                .unwrap();
        });

        assert_eq!(
            probe(server_addr, Duration::from_secs(1)).await.unwrap(),
            Result::Responsive
        );
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_oversized_truncated_response() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();

        let responder = tokio::spawn(async move {
            let mut packet = [0u8; REQUEST_SIZE];
            let (len, peer) = server.recv_from(&mut packet).await.unwrap();
            let request = ProbeRequest::from_bytes(&packet[..len]).unwrap();
            let mut oversized = [0u8; RECEIVE_BUFFER_SIZE + 1];
            oversized[..RESPONSE_SIZE].copy_from_slice(request.response().as_bytes());
            server.send_to(&oversized, peer).await.unwrap();
        });

        assert_eq!(
            probe(server_addr, Duration::from_millis(250))
                .await
                .unwrap(),
            Result::Unresponsive
        );
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn times_out_when_peer_does_not_respond() {
        let sink = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sink_addr = sink.local_addr().unwrap();

        assert_eq!(
            probe(sink_addr, Duration::from_millis(10)).await.unwrap(),
            Result::Unresponsive
        );
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{no_tls::NoTlsProvider, *};
use s2n_codec::EncoderBuffer;
use s2n_quic::connection::Error;
use s2n_quic_core::{
    event::api::Subject,
    packet::interceptor::{Datagram, Interceptor},
    transport,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The Key Phase bit within the first byte of a short header packet
const KEY_PHASE_BIT: u8 = 0x04;

/// 1-RTT datagrams the client sends before the simulated key update begins
///
/// This only needs to be large enough for the server to authenticate a packet, which establishes
/// the packet number that the replayed packet is compared against.
const DATAGRAMS_BEFORE_UPDATE: usize = 3;

/// Returns `true` if the datagram begins with a 1-RTT (short header) packet
///
/// Short headers have the header form bit clear and the fixed bit set.
fn is_short_header(payload: &[u8]) -> bool {
    matches!(payload.first(), Some(first) if first & 0b1100_0000 == 0b0100_0000)
}

/// Drives the key update scenario
///
/// The same value fills three roles so that the shared state stays consistent without any
/// additional plumbing:
///
/// * an [`Interceptor`] on the client, which simulates the client performing a key update
/// * a [`Network`](io::Network) wrapped around the [`Model`], which captures and replays a packet
/// * an event subscriber on the server, which records the key phase rotations the server performs
#[derive(Clone, Default)]
struct KeyPhaseReplay {
    /// The address of the server, used to select the client to server direction
    server_addr: Arc<Mutex<Option<SocketAddr>>>,
    /// 1-RTT datagrams the client has sent
    sent: Arc<AtomicUsize>,
    /// A captured pre-update datagram, replayed once the server has rotated its key phase
    captured: Arc<Mutex<Option<Packet>>>,
    /// Whether the captured datagram has been replayed
    replayed: Arc<AtomicBool>,
    /// The generation of each 1-RTT key phase rotation the server performed
    server_rotations: Arc<Mutex<Vec<u16>>>,
}

impl KeyPhaseReplay {
    /// Returns `true` once the server has responded to the simulated key update
    fn server_rotated(&self) -> bool {
        !self.server_rotations.lock().unwrap().is_empty()
    }

    fn is_to_server(&self, packet: &Packet) -> bool {
        let destination: SocketAddr = packet.path.remote_address.0.into();
        *self.server_addr.lock().unwrap() == Some(destination)
    }
}

/// Simulates the client performing a key update, from the server's point of view
impl Interceptor for KeyPhaseReplay {
    fn intercept_tx_datagram(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: &mut EncoderBuffer,
    ) {
        let payload = payload.as_mut_slice();

        // Coalesced handshake packets are left alone; only 1-RTT packets carry a Key Phase.
        if !is_short_header(payload) {
            return;
        }

        if self.sent.fetch_add(1, Ordering::SeqCst) < DATAGRAMS_BEFORE_UPDATE {
            return;
        }

        // The bit is set rather than toggled. The client rotates its own phase in response to the
        // server's rotation, and forcing the bit keeps the wire consistent either way.
        payload[0] |= KEY_PHASE_BIT;
    }
}

// Set up the simulated network to replay a captured datagram.
impl io::Network for KeyPhaseReplay {
    fn execute(&mut self, buffers: &io::network::Buffers) -> usize {
        if self.replayed.load(Ordering::SeqCst) {
            return 0;
        }

        {
            // Keep a copy of the client's packet so it be ingected again into the network later.
            let mut captured = self.captured.lock().unwrap();
            buffers.pending_transmission(|packet| {
                if self.is_to_server(packet)
                    && is_short_header(&packet.payload)
                    && packet.payload[0] & KEY_PHASE_BIT == 0
                {
                    *captured = Some(packet.clone());
                }
            });
        }

        // replay only once the server has rotated, while it still holds the previous receive key
        if !self.server_rotated() {
            return 0;
        }

        let Some(mut packet) = self.captured.lock().unwrap().take() else {
            return 0;
        };

        // deliver like the `Model` does, but with no delay so the derivation timer is still armed
        packet.switch();
        buffers.rx(*packet.path.local_address, |queue| queue.enqueue(packet));
        self.replayed.store(true, Ordering::SeqCst);

        1
    }
}

/// Records the 1-RTT key phase rotations an endpoint performs
impl events::Subscriber for KeyPhaseReplay {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_key_update(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &events::ConnectionMeta,
        event: &events::KeyUpdate,
    ) {
        // generation 0 is published when the 1-RTT keys are first installed
        if let events::KeyType::OneRtt { generation, .. } = event.key_type {
            if generation > 0 {
                self.server_rotations.lock().unwrap().push(generation);
            }
        }
    }
}

// Previous receive keys are retained for about a PTO after a key update (RFC 9001 6.5), so the Key
// Phase bit cannot distinguish the previous phase from the next one; the packet number is what
// disambiguates them. Rotating on the bit alone rewinds the endpoint's send keys to the retired
// generation as soon as a delayed or replayed packet from the previous phase arrives.
//
// The interceptor sets the bit from the fourth client packet on, so the server rotates and retains
// its previous key. The network then replays a datagram from before that point: it carries the
// previous Key Phase and an already processed packet number, so the server must accept it and drop it
// as a duplicate without touching any key state.
#[test]
fn replayed_previous_key_phase_does_not_rotate_keys() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(10));

    let scenario = KeyPhaseReplay::default();

    // the scenario runs before the model so that it observes packets before they are drained
    test((scenario.clone(), model.clone()), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            // Both tests use the null TLS provider because they forge the Key Phase bit on the wire, which
            // requires no header protection. The forged bit is used for a genuine key update.
            .with_tls(NoTlsProvider::default())?
            .with_event((tracing_events(false, model.clone()), scenario.clone()))?
            .with_random(Random::with_seed(456))?
            .start()?;
        let addr = start_server(server)?;
        *scenario.server_addr.lock().unwrap() = Some(addr);

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::default())?
            .with_event(tracing_events(false, model.clone()))?
            .with_random(Random::with_seed(123))?
            .with_packet_interceptor(scenario.clone())?
            .start()?;

        // the transfer asserts that every byte arrives in both directions, so the connection has
        // to survive the replay for the simulation to finish
        start_client(client, addr, Data::new(100_000))?;

        Ok(addr)
    })
    .unwrap();

    assert!(
        scenario.replayed.load(Ordering::SeqCst),
        "the scenario never replayed a packet, so nothing was verified"
    );

    // The simulated key update accounts for the single rotation. The replayed packet from the
    // previous phase must not produce another one.
    let rotations = scenario.server_rotations.lock().unwrap();
    assert_eq!(
        &rotations[..],
        &[1],
        "the server should rotate its key phase exactly once"
    );
}

/// Sets the Key Phase bit on a single 1-RTT datagram
///
/// The peer rotates in response, and every packet after it then carries the previous Key Phase with
/// a higher packet number, which is the condition RFC 9001 6.4 requires an endpoint to reject.
#[derive(Clone, Default)]
struct SingleKeyPhaseFlip {
    sent: Arc<AtomicUsize>,
}

impl Interceptor for SingleKeyPhaseFlip {
    fn intercept_tx_datagram(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: &mut EncoderBuffer,
    ) {
        let payload = payload.as_mut_slice();

        if !is_short_header(payload) {
            return;
        }

        if self.sent.fetch_add(1, Ordering::SeqCst) == DATAGRAMS_BEFORE_UPDATE {
            payload[0] |= KEY_PHASE_BIT;
        }
    }
}

// RFC 9001 6.4 requires a KEY_UPDATE_ERROR when protection is successfully removed with old keys from
// a packet numbered higher than one protected with newer keys. That check was missing.
//
// Setting the bit on a single datagram produces the condition: the server rotates, and every packet
// after it carries the previous Key Phase with a higher packet number.
#[test]
fn previous_key_phase_with_higher_packet_number_is_a_key_update_error() {
    let closed = recorder::ConnectionClosed::new();
    let errors = closed.events();

    let model = Model::default();
    model.set_delay(Duration::from_millis(10));

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::default())?
            .with_event((tracing_events(false, model.clone()), closed))?
            .with_random(Random::with_seed(456))?
            .start()?;
        let addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::default())?
            .with_event(tracing_events(false, model.clone()))?
            .with_random(Random::with_seed(123))?
            .with_packet_interceptor(SingleKeyPhaseFlip::default())?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();
            let mut stream = connection.open_bidirectional_stream().await.unwrap();

            // the server closes the connection partway through, so the transfer is expected to fail
            let mut data = Data::new(100_000);
            while let Some(chunk) = data.send_one(usize::MAX) {
                if stream.send(chunk).await.is_err() {
                    return;
                }
            }
            let _ = stream.flush().await;
        });

        Ok(addr)
    })
    .unwrap();

    let errors = errors.lock().unwrap();
    let Some(Error::Transport { code, .. }) = errors.first() else {
        panic!("expected the server to close with a transport error, got {errors:?}");
    };
    assert_eq!(*code, transport::Error::KEY_UPDATE_ERROR.code);
}

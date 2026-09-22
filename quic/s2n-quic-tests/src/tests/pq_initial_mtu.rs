// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This file contains tests for handshake latency with post-quantum key exchange, which increases
//! the size of ClientHello / ServerHello. We found an unexpected >100ms latency regression due to
//! interaction of these packets with `initial_mtu = 9kb` set in s2n-quic-dc on pathways that can't
//! support that MTU. These tests help track that regression and our fixes for it.

use super::*;
use s2n_quic::provider::tls::default::{self as tls, security};

const BASE_MTU: u16 = 1450;
const JUMBO_MTU: u16 = 8940;

const RTT: Duration = Duration::from_millis(1);

/// A packet buffer large enough to hold a full jumbo datagram, as s2n-quic-dc configures.
const PACKET_BUFFER: u32 = JUMBO_MTU as u32;
/// s2n-quic's default: no buffering, so packets that arrive before their keys are dropped.
const NO_PACKET_BUFFER: u32 = 0;

/// A policy with no PQ key exchange, producing a ~271 byte ClientHello.
const CLASSICAL_POLICY: &str = "20240503";
/// A policy offering x25519_mlkem768, producing a ~1503 byte ClientHello.
const ML_KEM_POLICY: &str = "20250721";

/// The MTU configuration of a single endpoint, mirroring the io builder's setters.
#[derive(Clone, Copy)]
struct Mtu {
    base_mtu: u16,
    initial_mtu: u16,
    max_mtu: u16,
}

/// A conservative base with a jumbo initial and max, so the first flight is padded up to
/// `JUMBO_MTU`. This is what s2n-quic-dc ran before it disabled MTU discovery, and is still
/// what any application gets by configuring an `initial_mtu` the path cannot carry.
const JUMBO_INITIAL_MTU: Mtu = Mtu {
    base_mtu: BASE_MTU,
    initial_mtu: JUMBO_MTU,
    max_mtu: JUMBO_MTU,
};

/// s2n-quic-dc's configuration today: MTU discovery disabled, so it never sends a datagram
/// a 1450-byte path cannot carry.
const NO_MTU_DISCOVERY: Mtu = Mtu {
    base_mtu: BASE_MTU,
    initial_mtu: BASE_MTU,
    max_mtu: BASE_MTU,
};

/// One simulated handshake. Written out in full at each call site so that every knob a
/// case depends on is visible in the case itself.
#[derive(Clone, Copy)]
struct Scenario<'a> {
    /// s2n-tls security policy version, applied to both endpoints.
    policy_version: &'a str,
    /// Largest UDP payload the simulated network will deliver.
    path_mtu: u16,
    client_mtu: Mtu,
    server_mtu: Mtu,
    /// Size of the client's packet buffer, which holds handshake packets that arrive before
    /// the keys needed to decrypt them are available. `0` disables buffering, dropping them.
    client_packet_buffer: u32,
}

impl Scenario<'_> {
    /// Runs the handshake and returns how long it took in simulated time.
    fn handshake_time(&self) -> Duration {
        let model = Model::default();
        model.set_max_udp_payload(self.path_mtu);
        model.set_delay(RTT / 2);

        let result = Arc::new(Mutex::new(None));
        let handshake = result.clone();

        let policy = security::Policy::from_version(self.policy_version).unwrap();
        let (client_mtu, server_mtu) = (self.client_mtu, self.server_mtu);
        let client_packet_buffer = self.client_packet_buffer;

        test(model.clone(), |handle| {
            let server = tls::Server::from_loader({
                let mut builder = tls::config::Config::builder();
                builder
                    .enable_quic()?
                    .set_application_protocol_preference(["h3"])?
                    .set_security_policy(&policy)?
                    .load_pem(
                        certificates::CERT_PEM.as_bytes(),
                        certificates::KEY_PEM.as_bytes(),
                    )?;
                builder.build()?
            });

            let server = Server::builder()
                .with_io(
                    handle
                        .builder()
                        .with_base_mtu(server_mtu.base_mtu)
                        .with_initial_mtu(server_mtu.initial_mtu)
                        .with_max_mtu(server_mtu.max_mtu)
                        .build()?,
                )?
                .with_tls(server)?
                // The oversized first flight is dropped by the network, which this
                // harness would otherwise treat as a fatal event.
                .with_event(tracing_events(false, model.clone()))?
                .with_random(Random::with_seed(456))?
                .with_limits(
                    provider::limits::Limits::default().with_initial_round_trip_time(RTT)?,
                )?
                .start()?;

            let client = tls::Client::from_loader({
                let mut builder = tls::config::Config::builder();
                builder
                    .enable_quic()?
                    .set_application_protocol_preference(["h3"])?
                    .set_security_policy(&policy)?
                    .trust_pem(certificates::CERT_PEM.as_bytes())?;
                builder.build()?
            });

            let client = Client::builder()
                .with_io(
                    handle
                        .builder()
                        .with_base_mtu(client_mtu.base_mtu)
                        .with_initial_mtu(client_mtu.initial_mtu)
                        .with_max_mtu(client_mtu.max_mtu)
                        .build()?,
                )?
                .with_tls(client)?
                .with_event(tracing_events(false, model.clone()))?
                .with_random(Random::with_seed(456))?
                .with_limits(
                    provider::limits::Limits::default()
                        .with_initial_round_trip_time(RTT)?
                        // dc's client sets this, so that a handshake flight arriving before
                        // its keys can be derived is buffered rather than dropped.
                        .with_packet_buffer_size(client_packet_buffer)?,
                )?
                .start()?;

            let addr = start_server(server)?;

            primary::spawn(async move {
                let start = io::time::now();
                let connection = client
                    .connect(Connect::new(addr).with_server_name("localhost"))
                    .await;
                let elapsed = io::time::now() - start;
                *handshake.lock().unwrap() = Some((elapsed, connection.is_ok()));
            });

            Ok(addr)
        })
        .unwrap();

        let (elapsed, succeeded) = result.lock().unwrap().expect("handshake did not finish");
        assert!(succeeded, "handshake failed after {elapsed:?}");
        elapsed
    }
}

/// On a path that cannot carry the oversized first flight, an ML-KEM ClientHello takes far
/// longer to complete than a classical one, because no PTO probe can carry it and the
/// handshake waits out the client's PTO backoff ladder instead.
///
/// Each client probe delivers only a 1145-byte prefix of the 1503-byte ClientHello. The
/// server cannot report what it received, because its `Normal`-mode transmissions are padded
/// to `initial_mtu` too, so even a pure-ACK reply goes out as an 8912-byte datagram and is
/// dropped. Nothing is ever ACKed, so the client re-sends from offset 0 on every probe. In
/// the trace the client probes at 3, 9, 21, 45, 93 and 189ms — its PTO ladder of 3, 6, 12,
/// 24, 48, 96ms — and only when the server's own PTO fires at 189.5ms does a clamped,
/// deliverable ACK reach the client. It immediately sends `offset 1145..1503` and the
/// handshake completes at ~191ms.
#[test]
fn ml_kem_client_hello_exceeds_pto_probe() {
    // ~10ms: one PTO to recover the lost first flight in each direction.
    let classical = Scenario {
        policy_version: CLASSICAL_POLICY,
        path_mtu: 1500,
        client_mtu: JUMBO_INITIAL_MTU,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    // ~191ms: the client's PTO ladder has to reach its sixth expiry.
    let ml_kem = Scenario {
        policy_version: ML_KEM_POLICY,
        path_mtu: 1500,
        client_mtu: JUMBO_INITIAL_MTU,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    assert!(
        ml_kem > classical * 5,
        "expected a large ML-KEM regression, got classical={classical:?} ml_kem={ml_kem:?}"
    );
    assert!(
        ml_kem > Duration::from_millis(150),
        "expected the PTO backoff ladder to dominate, got {ml_kem:?}"
    );
}

/// The regression is caused by the first flight being padded above the path MTU, not by the
/// size of the ClientHello itself: when the path can carry the jumbo first flight, both
/// policies complete in a single round trip.
#[test]
fn no_regression_when_path_supports_initial_mtu() {
    let classical = Scenario {
        policy_version: CLASSICAL_POLICY,
        path_mtu: 9001,
        client_mtu: JUMBO_INITIAL_MTU,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    let ml_kem = Scenario {
        policy_version: ML_KEM_POLICY,
        path_mtu: 9001,
        client_mtu: JUMBO_INITIAL_MTU,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    assert_eq!(classical, ml_kem);
    assert!(ml_kem < RTT * 2, "expected ~1 RTT, got {ml_kem:?}");
}

/// With MTU discovery disabled on both endpoints no first flight is padded above the path MTU, so
/// nothing is lost and the PQ handshake costs exactly what the classical one does.
#[test]
fn no_jumbo_config_is_unaffected() {
    let classical = Scenario {
        policy_version: CLASSICAL_POLICY,
        path_mtu: 1500,
        client_mtu: NO_MTU_DISCOVERY,
        server_mtu: NO_MTU_DISCOVERY,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    let ml_kem = Scenario {
        policy_version: ML_KEM_POLICY,
        path_mtu: 1500,
        client_mtu: NO_MTU_DISCOVERY,
        server_mtu: NO_MTU_DISCOVERY,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    assert_eq!(classical, ml_kem);
    assert!(ml_kem < RTT * 2, "expected ~1 RTT, got {ml_kem:?}");
}

/// This test confirms that we get reasonable behavior from only deploying the fixes client side
/// (while server remains unchanged).
#[test]
fn client_initial_mtu_at_base_avoids_pto_ladder() {
    let client_mtu = Mtu {
        base_mtu: BASE_MTU,
        initial_mtu: BASE_MTU,
        max_mtu: JUMBO_MTU,
    };

    // ~4ms: 1 RTT plus one PTO for the server's dropped jumbo first flight.
    let classical = Scenario {
        policy_version: CLASSICAL_POLICY,
        path_mtu: 1500,
        client_mtu,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    // ~5ms: one further round trip for the tail of the server's oversized flight.
    let ml_kem = Scenario {
        policy_version: ML_KEM_POLICY,
        path_mtu: 1500,
        client_mtu,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    assert!(
        ml_kem == RTT + classical,
        "classical={classical:?} ml_kem={ml_kem:?}"
    );
    assert!(ml_kem < Duration::from_millis(20), "{ml_kem:?}");
}

/// Client-side packet buffering saves round trips for an ML-KEM handshake against an
/// unchanged (jumbo `initial_mtu`) server: without it, handshake packets that arrive before
/// the client can derive handshake keys are dropped and must be retransmitted.
#[test]
fn client_packet_buffering_saves_round_trips() {
    let client_mtu = Mtu {
        base_mtu: BASE_MTU,
        initial_mtu: BASE_MTU,
        max_mtu: JUMBO_MTU,
    };

    // ~9ms without buffering.
    let unbuffered = Scenario {
        policy_version: ML_KEM_POLICY,
        path_mtu: 1500,
        client_mtu,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: NO_PACKET_BUFFER,
    }
    .handshake_time();

    // ~5ms with buffering.
    let buffered = Scenario {
        policy_version: ML_KEM_POLICY,
        path_mtu: 1500,
        client_mtu,
        server_mtu: JUMBO_INITIAL_MTU,
        client_packet_buffer: PACKET_BUFFER,
    }
    .handshake_time();

    assert!(
        unbuffered == buffered + RTT * 4,
        "expected buffering to save 4 RTTs, got unbuffered={unbuffered:?} buffered={buffered:?}"
    );
}

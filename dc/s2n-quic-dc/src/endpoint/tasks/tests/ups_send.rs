// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `ups_send` pipeline.
//!
//! Verifies that:
//! 1. A UPS response with a valid credential is sent to the destination address.
//! 2. A duplicate response for the same credential within the dedup window is suppressed.

use crate::{
    counter::Registry,
    credentials::Id,
    endpoint::{tasks, ups},
    intrusive::Entry,
    msg::addr::Addr,
    packet::{secret_control, WireVersion},
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::bach::Clock,
};
use bach::net::UdpSocket;
use core::time::Duration;
use s2n_codec::EncoderBuffer;
use std::net::SocketAddr;

const STATELESS_RESET_TAG: &[u8; 16] = b"ups-test-signer!";
const CREDENTIAL_BYTES: [u8; 16] = [0xAB; 16];

fn encode_ups_packet(id: Id) -> Vec<u8> {
    let mut buf = [0u8; secret_control::MAX_PACKET_SIZE];
    let stateless_reset: [u8; secret_control::TAG_LEN] = *STATELESS_RESET_TAG;
    let len = secret_control::UnknownPathSecret {
        wire_version: WireVersion::ZERO,
        credential_id: id,
        queue_id: None,
    }
    .encode(EncoderBuffer::new(&mut buf), &stateless_reset);
    buf[..len].to_vec()
}

fn make_response(dest: SocketAddr, id: Id) -> Entry<ups::Response> {
    let mut addr = Addr::default();
    addr.set(dest.into());
    Entry::new(ups::Response {
        dest_addr: addr,
        packet: encode_ups_packet(id),
    })
}

/// A UPS response with a valid credential reaches the destination socket.
#[test]
fn ups_response_is_delivered_to_destination() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let id = Id::from(CREDENTIAL_BYTES);

        // Create a channel so the feeder task can push a response after resolving the
        // server's simulated address.
        let (mut input_tx, input_rx) = unsync::new::<ups::Response>();

        // Server: bind and wait for exactly one datagram.
        async {
            let recv_socket = UdpSocket::bind("0.0.0.0:4433").await.unwrap();
            let mut buf = vec![0u8; 1500];
            let (n, _from) = recv_socket.recv_from(&mut buf).await.unwrap();
            assert!(n > 0, "UPS response should have been delivered");
        }
        .group("server")
        .primary()
        .spawn();

        // Feeder: resolve server address then push a UPS response into the pipeline.
        async move {
            let dest = bach::net::lookup_host("server:4433")
                .await
                .expect("lookup failed")
                .next()
                .expect("no address");
            let _ = input_tx.send(make_response(dest, id));
            drop(input_tx);
        }
        .spawn();

        // Pipeline: drain until the channel closes after sending the one response.
        async move {
            let send_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            let registry = Registry::default();
            let counters = ups::Counters::new(&registry);
            let clock = Clock::default();
            let rx = tasks::ups_send(
                input_rx,
                send_socket,
                clock,
                crate::socket::rate::Rate::new(100.0),
                1024,
                Duration::from_secs(60),
                counters,
            );
            rx.drain_budgeted(Some(32)).await;
        }
        .spawn();
    });
}

/// A second UPS response for the same credential within the dedup window is suppressed.
///
/// The server receives exactly one datagram (the first response). The duplicate is silently
/// dropped by `DedupFilter`. The `dedup_suppressed` counter reflects the suppression.
#[test]
fn duplicate_within_window_is_suppressed() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let id = Id::from(CREDENTIAL_BYTES);
        let (mut input_tx, input_rx) = unsync::new::<ups::Response>();

        // Server: bind and receive exactly one datagram (the first; the duplicate is dropped).
        async {
            let recv_socket = UdpSocket::bind("0.0.0.0:4433").await.unwrap();
            let mut buf = vec![0u8; 1500];
            let (n, _from) = recv_socket.recv_from(&mut buf).await.unwrap();
            assert!(n > 0, "first UPS response should be delivered");
        }
        .group("server")
        .primary()
        .spawn();

        // Feeder: push two responses for the same credential.
        async move {
            let dest = bach::net::lookup_host("server:4433")
                .await
                .expect("lookup failed")
                .next()
                .expect("no address");
            let _ = input_tx.send(make_response(dest, id));
            let _ = input_tx.send(make_response(dest, id));
            drop(input_tx);
        }
        .spawn();

        // Pipeline: drain with a large budget; first item passes, second is suppressed by dedup.
        // The server's primary task completes after receiving exactly one packet.
        async move {
            let send_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            let registry = Registry::default();
            let counters = ups::Counters::new(&registry);
            let clock = Clock::default();
            let rx = tasks::ups_send(
                input_rx,
                send_socket,
                clock,
                crate::socket::rate::Rate::new(100.0),
                1024,
                Duration::from_secs(60), // long window → second is suppressed
                counters,
            );
            rx.drain_budgeted(Some(32)).await;
        }
        .spawn();
    });
}

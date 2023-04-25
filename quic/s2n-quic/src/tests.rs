// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::Connect,
    provider::{
        self,
        event::{
            events::{MtuUpdated, MtuUpdatedCause, PacketSent, RecoveryMetrics},
            ConnectionInfo, ConnectionMeta, Subscriber,
        },
        io::testing::{rand, spawn, test, time::delay, Model},
        packet_interceptor::Loss,
    },
    Client, Server,
};
use bytes::Bytes;
use s2n_quic_core::{crypto::tls::testing::certificates, stream::testing::Data};
use s2n_quic_platform::io::testing::{network::Packet, primary, TxRecorder};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod setup;
use setup::*;

#[test]
fn client_server_test() {
    test(Model::default(), client_server).unwrap();
}

fn blackhole(model: Model, blackhole_duration: Duration) {
    test(model.clone(), |handle| {
        spawn(async move {
            // switch back and forth between blackhole and not
            loop {
                delay(blackhole_duration).await;
                // drop all packets
                model.set_drop_rate(1.0);

                delay(blackhole_duration).await;
                model.set_drop_rate(0.0);
            }
        });
        client_server(handle)
    })
    .unwrap();
}

#[test]
fn blackhole_success_test() {
    let model = Model::default();

    let network_delay = Duration::from_millis(1000);
    model.set_delay(network_delay);

    // setting the blackhole time to `network_delay / 2` causes the connection to
    // succeed
    let blackhole_duration = network_delay / 2;
    blackhole(model, blackhole_duration);
}

#[test]
#[should_panic]
fn blackhole_failure_test() {
    let model = Model::default();

    let network_delay = Duration::from_millis(1000);
    model.set_delay(network_delay);

    // setting the blackhole time to `network_delay / 2 + 1` causes the connection to fail
    let blackhole_duration = network_delay / 2 + Duration::from_millis(1);
    blackhole(model, blackhole_duration);
}

fn intercept_loss(loss: Loss<Random>) {
    let model = Model::default();
    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(events())?
            .with_packet_interceptor(loss)?
            .start()?;
        let server_address = start_server(server)?;

        client(handle, server_address)
    })
    .unwrap();
}

#[test]
fn interceptor_success_test() {
    intercept_loss(
        Loss::builder(Random::with_seed(123))
            .with_rx_loss(0..20)
            .with_rx_pass(1..5)
            .build(),
    )
}

#[test]
#[should_panic]
fn interceptor_failure_test() {
    intercept_loss(
        Loss::builder(Random::with_seed(123))
            .with_rx_loss(0..20)
            .with_rx_pass(1..2)
            .build(),
    )
}

/// Ensures streams with STOP_SENDING are properly cleaned up
///
/// See https://github.com/aws/s2n-quic/pull/1361
#[test]
fn stream_reset_test() {
    let model = Model::default();
    test(model, |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(events())?
            .with_limits(
                provider::limits::Limits::default()
                    // only allow 1 concurrent stream form the peer
                    .with_max_open_local_bidirectional_streams(1)
                    .unwrap(),
            )?
            .start()?;
        let server_addr = server.local_addr()?;

        spawn(async move {
            while let Some(mut connection) = server.accept().await {
                spawn(async move {
                    while let Some(mut stream) =
                        connection.accept_bidirectional_stream().await.unwrap()
                    {
                        spawn(async move {
                            // drain the receive stream
                            while stream.receive().await.unwrap().is_some() {}

                            // send data until the client resets the stream
                            while stream.send(Bytes::from_static(&[42; 1024])).await.is_ok() {}
                        });
                    }
                });
            }
        });

        let client = build_client(handle)?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();

            for mut remaining_chunks in 0usize..4 {
                let mut stream = connection.open_bidirectional_stream().await.unwrap();

                primary::spawn(async move {
                    stream.send(Bytes::from_static(&[42])).await.unwrap();
                    stream.finish().unwrap();

                    loop {
                        stream.receive().await.unwrap().unwrap();
                        if let Some(next_value) = remaining_chunks.checked_sub(1) {
                            remaining_chunks = next_value;
                        } else {
                            let _ = stream.stop_sending(123u8.into());
                            break;
                        }
                    }
                });
            }
        });

        Ok(())
    })
    .unwrap();
}

/// Ensures tokio `AsyncRead` implementation functions properly
///
/// See https://github.com/aws/s2n-quic/issues/1427
#[test]
fn tokio_read_exact_test() {
    let model = Model::default();
    test(model, |handle| {
        let server_addr = server(handle)?;

        let client = build_client(handle)?;

        // send 5000 bytes
        const LEN: usize = 5000;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();
            let stream = connection.open_bidirectional_stream().await.unwrap();
            let (mut recv, mut send) = stream.split();

            primary::spawn(async move {
                let mut read_len = 0;
                let mut buf = [0u8; 1000];
                // try to read from the stream until we read LEN bytes
                while read_len < LEN {
                    let max_len = buf.len().min(LEN - read_len);
                    // generate a random amount of bytes to read
                    let len = rand::gen_range(1..=max_len);

                    let buf = &mut buf[0..len];
                    recv.read_exact(buf).await.unwrap();

                    // record the amount that was read
                    read_len += len;
                }
                assert_eq!(read_len, LEN);
            });

            let mut write_len = 0;
            let mut buf = &[42u8; LEN][..];
            while !buf.is_empty() {
                // split the `buf` until it's empty
                let chunk_len = write_len.min(buf.len());
                let (chunk, remaining) = buf.split_at(chunk_len);

                // ensure the chunk is written to the stream
                send.write_all(chunk).await.unwrap();

                buf = remaining;
                // slowly increase the size of the chunks written
                write_len += 1;

                // by slowing the rate at which we send, we exercise the receiver's buffering logic in `read_exact`
                delay(Duration::from_millis(10)).await;
            }
        });

        Ok(())
    })
    .unwrap();
}

/// Ensures the peer is notified of locally-created streams
///
/// # Client expectations
/// * The client connects to the server
/// * The client opens a bidirectional stream
/// * The client reads 100 bytes from the newly created stream
///
/// # Server expectations
/// * The server accepts a new connection
/// * The server accepts a new bidirectional stream
/// * The server writes 100 bytes to the newly accepted stream
///
/// Unless the client notifies the server of the stream creation, the connection
/// is dead-locked and will timeout.
///
/// See https://github.com/aws/s2n-quic/issues/1464
#[test]
fn local_stream_open_notify_test() {
    let model = Model::default();
    test(model, |handle| {
        let mut server = build_server(handle)?;
        let server_addr = server.local_addr()?;

        // send 100 bytes
        const LEN: usize = 100;

        spawn(async move {
            while let Some(mut conn) = server.accept().await {
                while let Ok(Some(mut stream)) = conn.accept_bidirectional_stream().await {
                    primary::spawn(async move {
                        stream.send(vec![42; LEN].into()).await.unwrap();
                    });
                }
            }
        });

        let client = build_client(handle)?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();
            let mut stream = connection.open_bidirectional_stream().await.unwrap();

            let mut recv_len = 0;
            while let Ok(Some(chunk)) = stream.receive().await {
                recv_len += chunk.len();
            }

            assert_eq!(LEN, recv_len);
        });

        Ok(())
    })
    .unwrap();
}

macro_rules! event_recorder {
    ($sub:ident, $event:ty, $method:ident) => {
        event_recorder!($sub, $event, $method, $event, {
            |event: &$event, storage: &mut Vec<$event>| storage.push(event.clone())
        });
    };
    ($sub:ident, $event:ty, $method:ident, $storage:ty, $store:expr) => {
        #[derive(Clone, Default)]
        struct $sub {
            events: Arc<Mutex<Vec<$storage>>>,
        }

        impl $sub {
            fn new() -> Self {
                Self::default()
            }

            fn events(&self) -> Arc<Mutex<Vec<$storage>>> {
                self.events.clone()
            }
        }

        impl Subscriber for $sub {
            type ConnectionContext = $sub;

            fn create_connection_context(
                &mut self,
                _meta: &ConnectionMeta,
                _info: &ConnectionInfo,
            ) -> Self::ConnectionContext {
                self.clone()
            }

            fn $method(
                &mut self,
                context: &mut Self::ConnectionContext,
                _meta: &ConnectionMeta,
                event: &$event,
            ) {
                let store = $store;
                let mut buffer = context.events.lock().unwrap();
                store(event, &mut buffer);
            }
        }
    };
}

event_recorder!(PacketSentRecorder, PacketSent, on_packet_sent);
event_recorder!(MtuUpdatedRecorder, MtuUpdated, on_mtu_updated);
event_recorder!(
    PathUpdatedRecorder,
    RecoveryMetrics,
    on_recovery_metrics,
    SocketAddr,
    |event: &RecoveryMetrics, storage: &mut Vec<SocketAddr>| {
        let addr: SocketAddr = event.path.local_addr.to_string().parse().unwrap();
        if storage.last().map_or(true, |prev| *prev != addr) {
            storage.push(addr);
        }
    }
);
event_recorder!(
    PtoRecorder,
    RecoveryMetrics,
    on_recovery_metrics,
    u32,
    |event: &RecoveryMetrics, storage: &mut Vec<u32>| {
        storage.push(event.pto_count);
    }
);

#[test]
fn packet_sent_event_test() {
    let recorder = TxRecorder::default();
    let network_packets = recorder.get_packets();
    let subscriber = PacketSentRecorder::new();
    let events = subscriber.events();
    let mut server_socket = None;
    test((recorder, Model::default()), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(subscriber)?
            .start()?;
        let addr = start_server(server)?;
        // store addr in exterior scope so we can use it to filter packets
        // after the test ends
        server_socket = Some(addr);
        client(handle, addr)?;
        Ok(addr)
    })
    .unwrap();

    let server_socket = server_socket.unwrap();
    let mut events = events.lock().unwrap();
    let mut server_tx_network_packets: Vec<Packet> = network_packets
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .filter(|p| {
            let local_socket: SocketAddr = p.path.local_address.0.into();
            local_socket == server_socket
        })
        .collect();

    // transmitted quic packets may be coalesced into a single datagram (network packet)
    // so it might be the case that network_packet[0] = quic_packet[0] + quic_packet[1]
    while let Some(server_packet) = server_tx_network_packets.pop() {
        let expected_len = server_packet.payload.len();

        let mut event_len = 0;
        while expected_len > event_len {
            event_len += events.pop().unwrap().packet_len;
        }

        assert_eq!(expected_len, event_len)
    }
    assert!(events.is_empty());
}

// Construct a simulation where a client sends some data, which the server echos
// back. The MtuUpdated events that the server experiences are recorded and
// returns at the end of the simulation.
fn mtu_updates(max_mtu: u16) -> Vec<MtuUpdated> {
    let model = Model::default();
    model.set_max_udp_payload(max_mtu);

    let subscriber = MtuUpdatedRecorder::new();
    let events = subscriber.events();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(subscriber)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(10_000_000))?;
        Ok(addr)
    })
    .unwrap();

    let events_handle = events.lock().unwrap();
    events_handle.clone()
}

// if we specify jumbo frames on the endpoint and the network supports them,
// then jumbo frames should be negotiated.
#[test]
fn mtu_probe_jumbo_frame_test() {
    let events = mtu_updates(9_001);

    // handshake is padded to 1200, so we should immediate have an mtu of 1200
    // since the handshake successfully completes
    let handshake_mtu = events[0].clone();
    assert_eq!(handshake_mtu.mtu, 1200);
    assert!(matches!(
        handshake_mtu.cause,
        MtuUpdatedCause::NewPath { .. }
    ));

    // we should then successfully probe for 1500 (minus headers = 1472)
    let first_probe = events[1].clone();
    assert_eq!(first_probe.mtu, 1472);

    // we binary search upwards 9001
    // this isn't the maximum mtu we'd find in practice, just the maximum mtu we
    // find with a payload of 10_000_000 bytes.
    let last_probe = events.last().unwrap();
    assert_eq!(last_probe.mtu, 8943);
}

// if we specify jumbo frames on the endpoint and the network does not support
// them, the connection should gracefully complete with a smaller mtu
#[test]
fn mtu_probe_jumbo_frame_unsupported_test() {
    let events = mtu_updates(1_500);
    let last_mtu = events.last().unwrap();
    // ETHERNET_MTU - UDP_HEADER_LEN - IPV4_HEADER_LEN
    assert_eq!(last_mtu.mtu, 1472);
}

// if we lose every packet during a round trip and then allow packets through,
// this is not determined to be an MTU black hole
#[test]
fn mtu_loss_no_blackhole() {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    let max_mtu = 9001;
    let subscriber = MtuUpdatedRecorder::new();
    let events = subscriber.events();

    model.set_delay(rtt / 2);
    model.set_max_udp_payload(max_mtu);

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(subscriber)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(certificates::CERT_PEM)?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(10_000_000))?;

        spawn(async move {
            // let all packets go through for 10 RTTs - this will reach the end of MTU probing
            model.set_drop_rate(0.0);
            delay(rtt * 10).await;

            // drop all packets for a single round trip
            model.set_drop_rate(1.0);
            delay(rtt * 1).await;

            // now let the rest of the packets through
            model.set_drop_rate(0.0);
        });

        Ok(addr)
    })
    .unwrap();

    // MTU remained jumbo despite the packet loss
    assert_eq!(8943, events.lock().unwrap().last().unwrap().mtu);
}

// if the MTU is decreased after an MTU probe previously raised the MTU for the path,
// we detect an MTU black hole and decrease the MTU to the minimum
#[test]
fn mtu_blackhole() {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    let max_mtu = 9001;
    let subscriber = MtuUpdatedRecorder::new();
    let events = subscriber.events();

    model.set_delay(rtt / 2);
    model.set_max_udp_payload(max_mtu);

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(subscriber)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().with_max_mtu(max_mtu).build()?)?
            .with_tls(certificates::CERT_PEM)?
            .start()?;
        let addr = start_server(server)?;
        // we need a large payload to allow for multiple rounds of MTU probing
        start_client(client, addr, Data::new(10_000_000))?;

        spawn(async move {
            // let all packets go through for 10 RTTs - this will reach the end of MTU probing
            model.set_drop_rate(0.0);
            delay(rtt * 10).await;

            // decrease the MTU to trigger a blackhole
            model.set_max_udp_payload(1200);
        });

        Ok(addr)
    })
    .unwrap();

    // MTU dropped to the minimum
    assert_eq!(1200, events.lock().unwrap().last().unwrap().mtu);
}

/// Ensures that the client's local path handle is updated after it receives a packet from the
/// server
///
/// See https://github.com/aws/s2n-quic/issues/954
#[test]
fn client_path_handle_update() {
    let model = Model::default();

    let subscriber = PathUpdatedRecorder::new();
    let events = subscriber.events();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event(subscriber)?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();

    let events_handle = events.lock().unwrap();

    // initially, the client address should be unknown
    assert_eq!(events_handle[0], "0.0.0.0:0".parse().unwrap());
    // after receiving a packet, the client port should be the first available ephemeral port
    assert_eq!(events_handle[1], "1.0.0.1:49153".parse().unwrap());
    // there should only be a single update to the path handle
    assert_eq!(events_handle.len(), 2);
}

#[test]
fn increasing_pto_count_under_loss() {
    let delay_time = Duration::from_millis(10);

    let model = Model::default();
    model.set_delay(delay_time);
    let subscriber = PtoRecorder::new();
    let pto_events = subscriber.events();

    test(model.clone(), |handle| {
        spawn(async move {
            // allow for 1 RTT worth of data and then drop all packet after
            // the client gets an initial ACK from the server
            delay(delay_time * 2).await;
            model.set_drop_rate(1.0);
        });

        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            if let Some(conn) = server.accept().await {
                delay(Duration::from_secs(10)).await;
                let _ = conn;
            }
        });

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event(subscriber)?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let conn = client.connect(connect).await.unwrap();

            delay(Duration::from_secs(10)).await;
            let _ = conn;
        });

        Ok(addr)
    })
    .unwrap();

    let mut pto_events = pto_events.lock().unwrap();

    // assert that sufficient recovery events were captured
    let pto_len = pto_events.len();
    assert!(pto_len > 10);
    // the last recovery event is fired after we discard the handshake space so ignore it
    pto_events.truncate(pto_len - 1);

    let pto_count: u32 = *pto_events
        .iter()
        .reduce(|prev, new| {
            // assert that the value is monotonically increasing
            assert!(new >= prev, "prev_value {}, new_value {}", prev, new);
            new
        })
        .unwrap();

    // assert that the final pto_count increased to some large value over the
    // duration of the test
    assert!(
        pto_count > 5,
        "delay: {:?}. pto_count: {}",
        delay_time,
        pto_count
    );
}

// TODO: https://github.com/aws/s2n-quic/issues/1726
//
// The rustls tls provider is used on windows and has different
// build options than s2n-tls. We should build the rustls provider with
// mTLS enabled and remove the below `cfg_attr(ignore)`.
#[cfg_attr(any(target_os = "windows"), ignore)]
#[test]
fn mtls() {
    use crate::provider::tls::default as s2n_tls;

    // Ensure connection is successful under different network conditions
    for delay_time in (500..=10_000).step_by(1000) {
        for drop_percent in (0..=90).step_by(10) {
            let delay_time = Duration::from_micros(delay_time);

            let model = Model::default();
            model.set_delay(delay_time);

            test(model.clone(), |handle| {
                spawn(async move {
                    // allow for 1 RTT worth of data before impairing the connection.
                    // Both the Client and Server have received initial packets at
                    // this point.
                    model.set_delay(delay_time * 2);

                    // simulate network impairment
                    let drop_rate = (drop_percent / 100).into();
                    model.set_drop_rate(drop_rate);
                    model.set_corrupt_rate(drop_rate);
                });

                let server_tls = s2n_tls::Server::builder()
                    .with_certificate(
                        certificates::MTLS_SERVER_CERT,
                        certificates::MTLS_SERVER_KEY,
                    )?
                    .with_client_authentication()?
                    .with_trusted_certificate(certificates::MTLS_CA_CERT)?
                    .build()?;

                let mut server = Server::builder()
                    .with_io(handle.builder().build()?)?
                    .with_event(events())?
                    .with_tls(server_tls)?
                    .start()?;

                let addr = server.local_addr()?;
                spawn(async move {
                    if let Some(conn) = server.accept().await {
                        delay(Duration::from_secs(10)).await;
                        let _ = conn;
                    }
                });

                let client_tls = s2n_tls::Client::builder()
                    .with_certificate(certificates::MTLS_CA_CERT)?
                    .with_client_identity(
                        certificates::MTLS_CLIENT_CERT,
                        certificates::MTLS_CLIENT_KEY,
                    )?
                    .build()?;
                let client = Client::builder()
                    .with_io(handle.builder().build().unwrap())?
                    .with_event(events())?
                    .with_tls(client_tls)?
                    .start()?;

                primary::spawn(async move {
                    let connect = Connect::new(addr).with_server_name("localhost");
                    let conn = client.connect(connect).await.unwrap();

                    delay(Duration::from_secs(10)).await;
                    let _ = conn;
                });

                Ok(addr)
            })
            .unwrap();
        }
    }
}

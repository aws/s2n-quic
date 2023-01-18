// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::event::{events::PacketSent, ConnectionInfo, ConnectionMeta, Subscriber};
use crate::{
    client::Connect,
    provider::{
        self,
        io::testing::{rand, spawn, test, time::delay, Model},
        packet_interceptor::Loss,
    },
    Server,
};
use std::net::SocketAddr;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

mod setup;
use bytes::Bytes;
use s2n_quic_platform::io::testing::network::Packet;
use s2n_quic_platform::io::testing::{primary, TxRecorder};
use setup::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        let server = server_with(handle, |io| {
            Ok(Server::builder()
                .with_io(io)?
                .with_tls(SERVER_CERTS)?
                .with_event(events())?
                .with_packet_interceptor(loss)?
                .start()?)
        })?;

        client(handle, server)
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
            .with_rx_pass(1..4)
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

pub struct PacketSentSubscriber {
    pub packet_sent: Arc<Mutex<Vec<PacketSent>>>,
}

pub struct PacketSentContext {
    packet_sent: Arc<Mutex<Vec<PacketSent>>>,
}

impl Subscriber for PacketSentSubscriber {
    type ConnectionContext = PacketSentContext;

    fn create_connection_context(
        &mut self,
        _meta: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        PacketSentContext {
            packet_sent: Arc::clone(&self.packet_sent),
        }
    }

    fn on_packet_sent(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &PacketSent,
    ) {
        let mut buffer = context.packet_sent.lock().unwrap();
        buffer.push(event.clone());
    }
}

#[test]
fn packet_sent_event_test() {
    let recorder = TxRecorder::default();
    let network_packets = recorder.get_packets();
    let subscriber = PacketSentSubscriber {
        packet_sent: Arc::new(Mutex::new(Vec::new())),
    };
    let events = Arc::clone(&subscriber.packet_sent);
    let mut server_socket = None;
    test((recorder, Model::default()), |handle| {
        let addr = server_with_subscriber(handle, subscriber)?;
        server_socket = Some(addr.clone());
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

    // tranmitted quic packets may be coalesced into a single datagram (network packet)
    // so it might be the case that network_packet[0] = quic_packet[0] + quic_packet[1]
    let mut event_len_sum = 0;
    while !server_tx_network_packets.is_empty() && !events.is_empty() {
        let tx_packet_len = server_tx_network_packets.last().unwrap().payload.len();
        let packet_sent_len =  events.last().unwrap().packet_len;
        if tx_packet_len == packet_sent_len + event_len_sum
        {
            server_tx_network_packets.pop();
            events.pop();
            event_len_sum = 0;
        } else {
            event_len_sum += events.pop().unwrap().packet_len;
        }
    }
    assert_eq!(event_len_sum, 0);
}

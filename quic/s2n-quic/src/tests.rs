// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::Connect,
    provider::{
        self,
        io::testing::{spawn, test, time::delay, Model},
        packet_interceptor::Loss,
    },
    Server,
};
use std::time::Duration;

mod setup;
use bytes::Bytes;
use s2n_quic_platform::io::testing::primary;
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

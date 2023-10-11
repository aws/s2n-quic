// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

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
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
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

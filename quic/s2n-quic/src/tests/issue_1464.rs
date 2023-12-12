// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

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
            // Delay for a second to allow expiring timers and packet acks to be cleared out
            delay(Duration::from_secs(1)).await;
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

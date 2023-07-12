// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

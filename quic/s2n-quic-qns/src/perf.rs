// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::Bytes;
use s2n_quic::stream::{ReceiveStream, SendStream};

/// Drains a receive stream
pub async fn handle_receive_stream(mut stream: ReceiveStream) -> Result<()> {
    let mut chunks: [_; 64] = core::array::from_fn(|_| Bytes::new());

    loop {
        let (len, is_open) = stream.receive_vectored(&mut chunks).await?;

        if !is_open {
            break;
        }

        for chunk in chunks[..len].iter_mut() {
            // discard chunks
            *chunk = Bytes::new();
        }
    }

    Ok(())
}

/// Sends a specified amount of data on a send stream
pub async fn handle_send_stream(mut stream: SendStream, len: u64) -> Result<()> {
    let mut chunks: [_; 64] = core::array::from_fn(|_| Bytes::new());

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#4.1
    //# Since the goal here is to measure the efficiency of the QUIC
    //# implementation and not any application protocol, the performance
    //# application layer should be as light-weight as possible.  To this
    //# end, the client and server application layer may use a single
    //# preallocated and initialized buffer that it queues to send when any
    //# payload needs to be sent out.
    let mut data = s2n_quic_core::stream::testing::Data::new(len);

    loop {
        match data.send(usize::MAX, &mut chunks) {
            Some(count) => {
                stream.send_vectored(&mut chunks[..count]).await?;
            }
            None => {
                stream.finish()?;
                break;
            }
        }
    }

    Ok(())
}

//= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.3.1
//# Every stream opened by the client uses the first 8 bytes of the
//# stream data to encode a 64-bit unsigned integer in network byte order
//# to indicate the length of data the client wishes the server to
//# respond with.
pub async fn write_stream_size(stream: &mut SendStream, len: u64) -> Result<()> {
    let size = len.to_be_bytes();
    let chunk = Bytes::copy_from_slice(&size);
    stream.send(chunk).await?;
    Ok(())
}

pub async fn read_stream_size(stream: &mut ReceiveStream) -> Result<(u64, Bytes)> {
    let mut chunk = Bytes::new();
    let mut offset = 0;
    let mut id = [0u8; core::mem::size_of::<u64>()];

    while offset < id.len() {
        if let Some(c) = stream.receive().await? {
            chunk = c;
        } else {
            return Err("every stream should be prefixed with a u64 length".into());
        }

        let needed_len = id.len() - offset;
        let len = chunk.len().min(needed_len);

        id[offset..offset + len].copy_from_slice(&chunk[..len]);
        offset += len;
        bytes::Buf::advance(&mut chunk, len);
    }

    let id = u64::from_be_bytes(id);

    Ok((id, chunk))
}

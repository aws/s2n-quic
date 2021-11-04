// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::Bytes;
use s2n_quic::stream::{ReceiveStream, SendStream};

pub async fn write_request(mut stream: SendStream, request: &String) -> Result<()> {
    static GET: Bytes = Bytes::from_static(b"GET ");
    static END_OF_REQUEST: Bytes = Bytes::from_static(b"\r\n");

    stream
        .send_vectored(&mut [
            GET.clone(),
            Bytes::copy_from_slice(request.as_bytes()),
            END_OF_REQUEST.clone(),
        ])
        .await?;

    stream.finish()?;

    Ok(())
}

pub async fn read_request(mut stream: ReceiveStream) -> Result<String> {
    let mut path = String::new();
    let mut chunks = vec![Bytes::new(), Bytes::new()];
    let mut total_chunks = 0;
    loop {
        // grow the chunks
        if chunks.len() == total_chunks {
            chunks.push(Bytes::new());
        }
        let (consumed, is_open) = stream.receive_vectored(&mut chunks[total_chunks..]).await?;
        total_chunks += consumed;
        if parse_h09_request(&chunks[..total_chunks], &mut path, is_open)? {
            return Ok(path);
        }
    }
}

fn parse_h09_request(chunks: &[Bytes], path: &mut String, is_open: bool) -> Result<bool> {
    let mut bytes = chunks.iter().flat_map(|chunk| chunk.iter().cloned());

    macro_rules! expect {
        ($char:literal) => {
            match bytes.next() {
                Some($char) => {}
                None if is_open => return Ok(false),
                _ => return Err("invalid request".into()),
            }
        };
    }

    expect!(b'G');
    expect!(b'E');
    expect!(b'T');
    expect!(b' ');
    expect!(b'/');

    loop {
        match bytes.next() {
            Some(c @ b'0'..=b'9') => path.push(c as char),
            Some(c @ b'a'..=b'z') => path.push(c as char),
            Some(c @ b'A'..=b'Z') => path.push(c as char),
            Some(b'.') => path.push('.'),
            Some(b'/') => path.push('/'),
            Some(b'-') => path.push('-'),
            Some(b'\n') | Some(b'\r') => return Ok(true),
            Some(c) => return Err(format!("invalid request {}", c as char).into()),
            None => return Ok(!is_open),
        }
    }
}

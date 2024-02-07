// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    file::{abs_path, File},
    Result,
};
use bytes::Bytes;
use futures::StreamExt;
use s2n_quic::{
    stream::{BidirectionalStream, ReceiveStream, SendStream},
    Connection,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::time::timeout;
use tracing::debug;

pub(crate) async fn handle_connection(mut connection: Connection, www_dir: Arc<Path>) {
    loop {
        match connection.accept_bidirectional_stream().await {
            Ok(Some(stream)) => {
                let www_dir = www_dir.clone();
                // spawn a task per stream
                tokio::spawn(async move {
                    if let Err(err) = handle_stream(stream, www_dir).await {
                        eprintln!("Stream error: {err:?}")
                    }
                });
            }
            Ok(None) => {
                return;
            }
            Err(err) => {
                eprintln!("error while accepting stream: {err}");
                return;
            }
        }
    }
}

async fn handle_stream(stream: BidirectionalStream, www_dir: Arc<Path>) -> Result<()> {
    let (rx_stream, mut tx_stream) = stream.split();
    let path = read_request(rx_stream).await?;

    if let Some(amount) = path.strip_prefix("_perf/").and_then(|v| v.parse().ok()) {
        return handle_perf_stream(amount, tx_stream).await;
    }

    let abs_path = abs_path(&path, &www_dir);
    let mut file = File::open(&abs_path).await?;
    loop {
        match timeout(Duration::from_secs(1), file.next()).await {
            Ok(Some(Ok(chunk))) => {
                let len = chunk.len();
                debug!(
                    "{:?} bytes ready to send on Stream({:?})",
                    len,
                    tx_stream.id()
                );
                tx_stream.send(chunk).await?;
                debug!("{:?} bytes sent on Stream({:?})", len, tx_stream.id());
            }
            Ok(Some(Err(err))) => {
                eprintln!("error opening {abs_path:?}");
                tx_stream.reset(1u32.into())?;
                return Err(err.into());
            }
            Ok(None) => {
                tx_stream.finish()?;
                return Ok(());
            }
            Err(_) => {
                eprintln!("timeout opening {abs_path:?}");
            }
        }
    }
}

async fn handle_perf_stream(amount: u64, mut stream: SendStream) -> Result<()> {
    let mut data = s2n_quic_core::stream::testing::Data::new(amount);

    while let Some(chunk) = data.send_one(usize::MAX) {
        stream.send(chunk).await?;
    }

    stream.finish()?;

    Ok(())
}

async fn read_request(mut stream: ReceiveStream) -> Result<String> {
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

    // reset the copied path in case this isn't the first time a path is being parsed
    path.clear();

    loop {
        match bytes.next() {
            Some(c @ b'0'..=b'9') => path.push(c as char),
            Some(c @ b'a'..=b'z') => path.push(c as char),
            Some(c @ b'A'..=b'Z') => path.push(c as char),
            Some(b'.') => path.push('.'),
            Some(b'/') => path.push('/'),
            Some(b'-') => path.push('-'),
            Some(b'_') => path.push('_'),
            Some(b'\n' | b'\r') => return Ok(true),
            // https://www.w3.org/Protocols/HTTP/AsImplemented.html
            // > The document address will consist of a single word (ie no spaces).
            // > If any further words are found on the request line, they MUST either be ignored,
            // > or else treated according to the full HTTP spec.
            Some(b' ') => return Ok(true),
            Some(c) => return Err(format!("invalid request {}", c as char).into()),
            None => return Ok(!is_open),
        }
    }
}

#[test]
fn parse_h09_request_test() {
    fn parse(chunks: &[&str]) -> Result<Option<String>> {
        let chunks: Vec<_> = chunks
            .iter()
            .map(|v| Bytes::copy_from_slice(v.as_bytes()))
            .collect();

        let mut path = String::new();

        for idx in 0..chunks.len() {
            let _ = parse_h09_request(&chunks[..idx], &mut path, true);
        }

        let result = parse_h09_request(&chunks, &mut path, false);

        result.map(|has_request| if has_request { Some(path) } else { None })
    }

    macro_rules! test {
        ([$($chunk:expr),* $(,)?], $expected:pat) => {{
            let result = parse(&[$($chunk),*]).unwrap();
            let result = result.as_deref();
            assert!(matches!(result, $expected), "{:?}", result);
        }}
    }

    assert!(parse(&[]).is_err());
    test!(["GET /"], Some(""));
    test!(["GET /abc"], Some("abc"));
    test!(["GET /abc/123"], Some("abc/123"));
    test!(["GET /CAPS/lower"], Some("CAPS/lower"));
    test!(["GET /abc\rextra stuff"], Some("abc"));
    test!(["G", "E", "T", " ", "/", "t", "E", "s", "T"], Some("tEsT"));
}

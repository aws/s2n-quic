// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::Buf;
use futures::future::try_join_all;
use http::Uri;
use s2n_quic::{client::Connect, Client};
use s2n_quic_h3::h3;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{fs::File, io::AsyncWriteExt, spawn};
use url::Url;

pub async fn create_connection<'a, R: IntoIterator<Item = &'a Url>>(
    client: Client,
    connect: Connect,
    requests: R,
    download_dir: Arc<Option<PathBuf>>,
    keep_alive: Option<Duration>,
) -> Result<()> {
    eprintln!("connecting to {connect:#}");
    let mut connection = client.connect(connect).await?;

    if keep_alive.is_some() {
        connection.keep_alive(true)?;
    }

    let (mut driver, send_request) =
        h3::client::new(s2n_quic_h3::Connection::new(connection)).await?;

    let mut streams = vec![];
    for request in requests {
        streams.push(spawn(create_stream(
            send_request.clone(),
            request.clone(),
            download_dir.clone(),
        )));
    }

    for result in try_join_all(streams).await? {
        // `try_join_all` should be returning an Err if any stream fails, but it
        // seems to just include the Err in the Vec of results. This will force
        // any Error to bubble up so it can be printed in the output.
        result?;
    }

    if let Some(keep_alive) = keep_alive {
        tokio::time::sleep(keep_alive).await;
    }

    driver.shutdown(0).await?;

    Ok(())
}

async fn create_stream<B: Buf, T: h3::quic::OpenStreams<B>>(
    send_request: h3::client::SendRequest<T, B>,
    request: Url,
    download_dir: Arc<Option<PathBuf>>,
) -> Result<()> {
    eprintln!("GET {request}");

    match create_stream_inner(send_request, request.clone(), download_dir).await {
        Ok(()) => {
            eprintln!("Request {request} completed successfully");
            Ok(())
        }
        Err(error) => {
            eprintln!("Request {request} failed: {error:?}");
            Err(error)
        }
    }
}

async fn create_stream_inner<B: Buf, T: h3::quic::OpenStreams<B>>(
    mut send_request: h3::client::SendRequest<T, B>,
    request: Url,
    download_dir: Arc<Option<PathBuf>>,
) -> Result<()> {
    let uri = request.to_string().parse::<Uri>().unwrap();
    let req = http::Request::builder().uri(uri).body(())?;
    let mut stream = send_request.send_request(req).await?;
    stream.finish().await?;

    let resp = stream.recv_response().await?;

    eprintln!("Response: {:?} {}", resp.version(), resp.status());
    eprintln!("Headers: {:#?}", resp.headers());

    if let Some(download_dir) = download_dir.as_ref() {
        if download_dir == Path::new("/dev/null") {
            while stream.recv_data().await?.is_some() {}
        } else {
            let mut abs_path = download_dir.to_path_buf();
            abs_path.push(Path::new(request.path().trim_start_matches('/')));
            let mut file = File::create(&abs_path).await?;
            while let Some(mut chunk) = stream.recv_data().await? {
                file.write_all_buf(&mut chunk).await?;
            }
            file.flush().await?;
        }
    };

    Ok(())
}

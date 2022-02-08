// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{h3, Result};
use bytes::Buf;
use futures::{future, future::try_join_all};
use http::Uri;
use hyperium_h3::client::SendRequest;
use s2n_quic::{client::Connect, Client};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{fs::File, io::AsyncWriteExt, spawn};
use url::Url;

pub async fn create_h3_connection<'a, R: IntoIterator<Item = &'a Url>>(
    client: Client,
    connect: Connect,
    requests: R,
    download_dir: Arc<Option<PathBuf>>,
    keep_alive: Option<Duration>,
) -> Result<()> {
    eprintln!("connecting to {:#}", connect);
    let mut connection = client.connect(connect).await?;

    if keep_alive.is_some() {
        connection.keep_alive(true)?;
    }

    let (mut driver, send_request) =
        hyperium_h3::client::new(h3::Connection::new(connection)).await?;

    let drive = tokio::spawn(async move {
        let _ = future::poll_fn(|cx| driver.poll_close(cx)).await;
        Ok::<_, ()>(())
    });

    let mut streams = vec![];
    for request in requests {
        streams.push(spawn(create_h3_stream(
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

    let _ = drive.await?;

    Ok(())
}

async fn create_h3_stream<B: Buf, T: hyperium_h3::quic::OpenStreams<B>>(
    send_request: SendRequest<T, B>,
    request: Url,
    download_dir: Arc<Option<PathBuf>>,
) -> Result<()> {
    eprintln!("GET {}", request);

    match create_h3_stream_inner(send_request, request.clone(), download_dir).await {
        Ok(()) => {
            eprintln!("Request {} completed successfully", request);
            Ok(())
        }
        Err(error) => {
            eprintln!("Request {} failed: {:?}", request, error);
            Err(error)
        }
    }
}

async fn create_h3_stream_inner<B: Buf, T: hyperium_h3::quic::OpenStreams<B>>(
    mut send_request: SendRequest<T, B>,
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
        let mut abs_path = download_dir.to_path_buf();
        abs_path.push(Path::new(request.path().trim_start_matches('/')));
        let mut file = File::create(&abs_path).await?;
        while let Some(mut chunk) = stream.recv_data().await? {
            file.write_all_buf(&mut chunk).await?;
        }
        file.flush().await?;
    };

    Ok(())
}

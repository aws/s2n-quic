// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::Bytes;
use futures::future::try_join_all;
use s2n_quic::{client::Connect, connection::Handle, stream::SendStream, Client};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{fs::File, io::AsyncWriteExt, spawn};
use url::Url;

pub(crate) async fn create_connection<'a, R: IntoIterator<Item = &'a Url>>(
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

    let mut streams = vec![];
    for request in requests {
        streams.push(spawn(create_stream(
            connection.handle(),
            request.path().to_string(),
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
        connection.keep_alive(false)?;
    }

    Ok(())
}

async fn create_stream(
    connection: Handle,
    request: String,
    download_dir: Arc<Option<PathBuf>>,
) -> Result<()> {
    eprintln!("GET {request}");

    match create_stream_inner(connection, &request, download_dir).await {
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

async fn create_stream_inner(
    mut connection: Handle,
    request: &str,
    download_dir: Arc<Option<PathBuf>>,
) -> Result<()> {
    let stream = connection.open_bidirectional_stream().await?;
    let (mut rx_stream, tx_stream) = stream.split();

    write_request(tx_stream, request).await?;

    if let Some(download_dir) = download_dir.as_ref() {
        if download_dir == Path::new("/dev/null") {
            crate::perf::handle_receive_stream(rx_stream).await?;
        } else {
            let mut abs_path = download_dir.to_path_buf();
            abs_path.push(Path::new(request.trim_start_matches('/')));
            let mut file = File::create(&abs_path).await?;
            tokio::io::copy(&mut rx_stream, &mut file).await?;
            file.flush().await?;
        }
    } else {
        let mut stdout = tokio::io::stdout();
        tokio::io::copy(&mut rx_stream, &mut stdout).await?;
        stdout.flush().await?;
    };

    Ok(())
}

async fn write_request(mut stream: SendStream, request: &str) -> Result<()> {
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

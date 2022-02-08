// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    file::{abs_path, File},
    Result,
};
use bytes::Bytes;
use futures::StreamExt;
use http::StatusCode;
use hyperium_h3::{quic::BidiStream, server::RequestStream};
use s2n_quic::Connection;
use s2n_quic_h3::{h3, hyperium_h3};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::time::timeout;

pub async fn handle_h3_connection(connection: Connection, www_dir: Arc<Path>) {
    let mut conn = hyperium_h3::server::Connection::new(h3::Connection::new(connection))
        .await
        .unwrap();

    while let Ok(Some((req, stream))) = conn.accept().await {
        let www_dir = www_dir.clone();
        tokio::spawn(async {
            if let Err(err) = handle_h3_stream(req, stream, www_dir).await {
                eprintln!("Stream error: {:?}", err)
            }
        });
    }
}

async fn handle_h3_stream<T>(
    req: http::Request<()>,
    mut stream: RequestStream<T, Bytes>,
    www_dir: Arc<Path>,
) -> Result<()>
where
    T: BidiStream<Bytes>,
{
    let abs_path = abs_path(req.uri().path(), &www_dir);
    let mut file = File::open(&abs_path).await?;
    let resp = http::Response::builder().status(StatusCode::OK).body(())?;

    stream.send_response(resp).await?;

    loop {
        match timeout(Duration::from_secs(1), file.next()).await {
            Ok(Some(Ok(chunk))) => {
                stream.send_data(chunk).await?;
            }
            Ok(Some(Err(err))) => {
                eprintln!("error opening {:?}", abs_path);
                stream
                    .send_response(
                        http::Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(())?,
                    )
                    .await?;
                return Err(err.into());
            }
            Ok(None) => {
                stream.finish().await?;
                return Ok(());
            }
            Err(_) => {
                eprintln!("timeout opening {:?}", abs_path);
            }
        }
    }
}

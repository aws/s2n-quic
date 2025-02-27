// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{events, CliRange};
use s2n_quic::{
    client::Connect,
    provider::{
        event::tracing::Subscriber as Tracing,
        io::testing::{primary, rand, spawn, time, Handle, Result},
    },
    Client, Server,
};
use s2n_quic_core::{crypto::tls::testing::certificates, stream::testing::Data};
use std::net::SocketAddr;

pub fn server(handle: &Handle, events: events::Events) -> Result<SocketAddr> {
    let mut server = Server::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls((certificates::CERT_PEM, certificates::KEY_PEM))?
        .with_event((events, Tracing::default()))?
        .start()?;
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            primary::spawn(async move {
                while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                    primary::spawn(async move {
                        while let Ok(Some(chunk)) = stream.receive().await {
                            let _ = chunk;
                        }
                    });
                }
            });
        }
    });

    Ok(server_addr)
}

pub fn client(
    handle: &Handle,
    events: events::Events,
    servers: &[SocketAddr],
    count: usize,
    delay: CliRange<humantime::Duration>,
    streams: CliRange<u32>,
    stream_data: CliRange<u64>,
) -> Result {
    let client = Client::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls(certificates::CERT_PEM)?
        .with_event((events, Tracing::default()))?
        .start()?;

    let mut total_delay = core::time::Duration::ZERO;

    for _ in 0..count {
        total_delay += delay.gen_duration();

        // pick a random server to connect to
        let server_addr_idx = rand::Any::any(&(0..servers.len()));
        let server_addr = servers[server_addr_idx];
        let delay = total_delay;

        let client = client.clone();
        primary::spawn(async move {
            if !delay.is_zero() {
                time::delay(delay).await;
            }

            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await?;

            for _ in 0..streams.gen() {
                let stream = connection.open_bidirectional_stream().await?;
                primary::spawn(async move {
                    let (mut recv, mut send) = stream.split();

                    let mut send_data = Data::new(stream_data.gen());

                    let mut recv_data = send_data;
                    primary::spawn(async move {
                        while let Some(chunk) = recv.receive().await? {
                            recv_data.receive(&[chunk]);
                        }

                        <s2n_quic::stream::Result<()>>::Ok(())
                    });

                    while let Some(chunk) = send_data.send_one(usize::MAX) {
                        send.send(chunk).await?;
                    }

                    <s2n_quic::stream::Result<()>>::Ok(())
                })
                .await?;
            }

            <s2n_quic::stream::Result<()>>::Ok(())
        });
    }

    Ok(())
}

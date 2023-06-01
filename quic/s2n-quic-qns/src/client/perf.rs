// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{perf, tls, Result};
use futures::future::try_join_all;
use s2n_quic::{client, provider::io, Client, Connection};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Perf {
    #[structopt(short, long, default_value = "127.0.0.1")]
    ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(short, long)]
    server_name: Option<String>,

    #[structopt(long)]
    ca: Option<PathBuf>,

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    application_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(long, default_value = "9000")]
    max_mtu: u16,

    #[structopt(short, long, default_value = "::")]
    local_ip: std::net::IpAddr,

    #[structopt(long, default_value)]
    send: u64,

    #[structopt(long, default_value)]
    receive: u64,

    #[structopt(flatten)]
    limits: perf::Limits,

    /// Logs statistics for the endpoint
    #[structopt(long)]
    stats: bool,
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        let mut client = self.client()?;

        let mut requests = vec![];

        // TODO support a richer connection strategy
        for _ in 0..self.connections.unwrap_or(1) {
            let mut connect = client::Connect::new((self.ip, self.port));
            if let Some(server_name) = self.server_name.as_deref() {
                connect = connect.with_server_name(server_name);
            } else {
                // TODO allow skipping setting the server_name
                connect = connect.with_server_name("localhost");
            }
            let connection = client.connect(connect).await?;

            requests.push(handle_connection(connection, self.send, self.receive));
        }

        try_join_all(requests).await?;
        client.wait_idle().await?;

        return Ok(());

        async fn handle_connection(
            mut connection: Connection,
            send: u64,
            receive: u64,
        ) -> Result<()> {
            if send == 0 && receive == 0 {
                return Ok(());
            }

            let stream = connection.open_bidirectional_stream().await?;
            let (receive_stream, mut send_stream) = stream.split();

            let s = tokio::spawn(async move {
                perf::write_stream_size(&mut send_stream, receive).await?;
                perf::handle_send_stream(send_stream, send).await?;
                <Result<()>>::Ok(())
            });

            let r = tokio::spawn(perf::handle_receive_stream(receive_stream));

            let (s, r) = tokio::try_join!(s, r)?;
            s?;
            r?;

            Ok(())
        }
    }

    fn client(&self) -> Result<Client> {
        let mut io_builder = io::Default::builder()
            .with_receive_address((self.local_ip, 0u16).into())?
            .with_max_mtu(self.max_mtu)?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let tls = s2n_quic::provider::tls::default::Client::builder()
            .with_certificate(tls::default::ca(self.ca.as_ref())?)?
            .with_application_protocols(self.application_protocols.iter().map(String::as_bytes))?
            .build()?;

        let subscriber = perf::Subscriber::default();

        if self.stats {
            subscriber.spawn(core::time::Duration::from_secs(1));
        }

        let subscriber = (
            subscriber,
            s2n_quic::provider::event::tracing::Subscriber::default(),
        );

        let client = Client::builder()
            .with_limits(self.limits.limits())?
            .with_io(io)?
            .with_event(subscriber)?
            .with_tls(tls)?
            .start()
            .unwrap();

        Ok(client)
    }
}

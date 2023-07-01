// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{perf, tls, Result};
use s2n_quic::{client, Client, Connection};
use structopt::StructOpt;
use tokio::task::JoinSet;

#[derive(Debug, StructOpt)]
pub struct Perf {
    #[structopt(short, long, default_value = "127.0.0.1")]
    ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(short, long)]
    server_name: Option<String>,

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    application_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(long, default_value)]
    send: u64,

    #[structopt(long, default_value)]
    receive: u64,

    #[structopt(long, default_value = "1")]
    streams: u64,

    #[structopt(flatten)]
    limits: perf::Limits,

    /// Logs statistics for the endpoint
    #[structopt(long)]
    stats: bool,

    #[structopt(flatten)]
    io: crate::io::Client,

    #[structopt(flatten)]
    tls: tls::Client,
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        let mut client = self.client()?;

        let mut requests = JoinSet::new();

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

            requests.spawn(handle_connection(
                connection,
                self.streams,
                self.send,
                self.receive,
            ));
        }

        // wait until all of the connections finish before closing
        while requests.join_next().await.is_some() {}

        client.wait_idle().await?;

        return Ok(());

        async fn handle_connection(
            mut connection: Connection,
            streams: u64,
            send: u64,
            receive: u64,
        ) -> Result<()> {
            if send == 0 && receive == 0 {
                return Ok(());
            }

            for _ in 0..streams {
                let stream = connection.open_bidirectional_stream().await?;
                let (receive_stream, mut send_stream) = stream.split();

                let s = async move {
                    perf::write_stream_size(&mut send_stream, receive).await?;
                    perf::handle_send_stream(send_stream, send).await?;
                    <Result<()>>::Ok(())
                };

                let r = perf::handle_receive_stream(receive_stream);

                let _ = tokio::try_join!(s, r)?;
            }

            Ok(())
        }
    }

    fn client(&self) -> Result<Client> {
        let io = self.io.build()?;

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
            .with_event(subscriber)?;

        let client = self.tls.build(client, &self.application_protocols)?;

        Ok(client)
    }
}

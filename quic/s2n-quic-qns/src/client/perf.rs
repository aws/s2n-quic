// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{client, perf, task, tls, Result};
use s2n_quic::{client::Connect, provider::event, Client, Connection};
use structopt::StructOpt;

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

    /// The total number of connections to open from the client
    #[structopt(long, default_value = "1")]
    connections: usize,

    /// Defines the number of concurrent connections to open at any given time
    #[structopt(long, default_value = "10")]
    concurrency: u64,

    #[structopt(long, default_value)]
    send: u64,

    #[structopt(long, default_value)]
    receive: u64,

    #[structopt(long, default_value = "1")]
    streams: u64,

    #[structopt(flatten)]
    limits: crate::limits::Limits,

    /// Logs statistics for the endpoint
    #[structopt(long)]
    stats: bool,

    #[structopt(flatten)]
    io: crate::io::Client,

    #[structopt(flatten)]
    tls: tls::Client,

    #[structopt(flatten)]
    runtime: crate::runtime::Runtime,

    #[structopt(flatten)]
    congestion_controller: crate::congestion_control::CongestionControl,
}

impl Perf {
    pub fn run(&self) -> Result<()> {
        self.runtime.build()?.block_on(self.task())
    }

    async fn task(&self) -> Result<()> {
        let mut client = self.client()?;

        let mut requests = task::Limiter::new(self.concurrency);

        let streams = self.streams;
        let send = self.send;
        let receive = self.receive;

        let mut connect = Connect::new((self.ip, self.port));
        if let Some(server_name) = self.server_name.as_deref() {
            connect = connect.with_server_name(server_name);
        } else {
            // TODO allow skipping setting the server_name
            connect = connect.with_server_name("localhost");
        }

        // TODO support a richer connection strategy
        for _ in 0..self.connections {
            let client = client.clone();
            let connect = connect.clone();

            let task = async move {
                let connection = client.connect(connect).await?;

                handle_connection(connection, streams, send, receive).await
            };

            let _ = requests.spawn(task).await;
        }

        while requests.join_next().await.is_none() {}

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

        let subscriber = event::console_perf::Builder::default()
            .with_format(event::console_perf::Format::TSV)
            .with_header(self.stats)
            .build();

        if self.stats {
            tokio::spawn({
                let mut subscriber = subscriber.clone();
                async move {
                    loop {
                        tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                        subscriber.print();
                    }
                }
            });
        }

        let subscriber = (subscriber, event::tracing::Subscriber::default());

        let client = Client::builder()
            .with_limits(self.limits.limits())?
            .with_io(io)?
            .with_event(subscriber)?;

        client::build(
            client,
            &self.application_protocols,
            &self.tls,
            &self.congestion_controller,
        )
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{perf, server, tls, Result};
use futures::future::try_join_all;
use s2n_quic::{
    provider::event,
    stream::{BidirectionalStream, ReceiveStream, SendStream},
    Connection, Server,
};
use structopt::StructOpt;
use tokio::spawn;

#[derive(Debug, StructOpt)]
pub struct Perf {
    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    application_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(flatten)]
    limits: crate::limits::Limits,

    /// Logs statistics for the endpoint
    #[structopt(long)]
    stats: bool,

    #[structopt(flatten)]
    tls: tls::Server,

    #[structopt(flatten)]
    io: crate::io::Server,

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
        let mut server = self.server()?;

        if let Some(limit) = self.connections {
            let mut connections = vec![];

            while connections.len() < limit {
                if let Some(connection) = server.accept().await {
                    // spawn a task per connection
                    connections.push(spawn(handle_connection(connection)));
                } else {
                    break;
                }
            }

            let did_panic = connections.len() != limit;

            try_join_all(connections).await?;

            println!("closing server after {limit} connections");

            if did_panic {
                return Err(crate::CRASH_ERROR_MESSAGE.into());
            }

            return Ok(());
        } else {
            while let Some(connection) = server.accept().await {
                // spawn a task per connection
                spawn(handle_connection(connection));
            }

            return Err(crate::CRASH_ERROR_MESSAGE.into());
        }

        async fn handle_connection(connection: Connection) {
            let (mut handle, acceptor) = connection.split();
            let (mut bidi, mut uni) = acceptor.split();

            let bidi = tokio::spawn(async move {
                loop {
                    match bidi.accept_bidirectional_stream().await? {
                        Some(stream) => {
                            // spawn a task per stream
                            tokio::spawn(async move {
                                //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.3.1
                                //# On the server side, any stream that is closed before all 8 bytes are
                                //# received should just be ignored, and gracefully closed on its end (if
                                //# applicable).
                                let _ = handle_bidi_stream(stream).await;
                            });
                        }
                        None => {
                            // the connection was closed without an error
                            return <Result<()>>::Ok(());
                        }
                    }
                }
            });

            let uni = tokio::spawn(async move {
                loop {
                    match uni.accept_receive_stream().await? {
                        Some(receiver) => {
                            let sender = handle.open_send_stream().await?;
                            // spawn a task per stream
                            tokio::spawn(async move {
                                //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.3.1
                                //# On the server side, any stream that is closed before all 8 bytes are
                                //# received should just be ignored, and gracefully closed on its end (if
                                //# applicable).
                                let _ = handle_uni_stream(receiver, sender).await;
                            });
                        }
                        None => {
                            // the connection was closed without an error
                            return <Result<()>>::Ok(());
                        }
                    }
                }
            });

            let _ = futures::try_join!(bidi, uni);
        }

        //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.3.2
        //# When a client uses a bidirectional stream to request a response
        //# payload from the server, the server sends the requested data on the
        //# same stream.  If no data is requested by the client, the server
        //# merely closes its side of the stream.
        async fn handle_bidi_stream(stream: BidirectionalStream) -> Result<()> {
            let (mut receiver, sender) = stream.split();
            let (size, _prelude) = perf::read_stream_size(&mut receiver).await?;

            let receiver = tokio::spawn(async move { perf::handle_receive_stream(receiver).await });
            let sender = tokio::spawn(async move { perf::handle_send_stream(sender, size).await });

            let _ = futures::try_join!(receiver, sender);

            Ok(())
        }

        //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.3.2
        //# When a client uses a unidirectional stream to request a response
        //# payload from the server, the server opens a new unidirectional stream
        //# to send the requested data.  If no data is requested by the client,
        //# the server need take no action.
        async fn handle_uni_stream(mut receiver: ReceiveStream, sender: SendStream) -> Result<()> {
            let (size, _prelude) = perf::read_stream_size(&mut receiver).await?;

            let receiver = tokio::spawn(async move { perf::handle_receive_stream(receiver).await });
            let sender = tokio::spawn(async move { perf::handle_send_stream(sender, size).await });

            let _ = futures::try_join!(receiver, sender);

            Ok(())
        }
    }

    fn server(&self) -> Result<Server> {
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

        let server = Server::builder()
            .with_limits(self.limits.limits())?
            .with_io(io)?
            .with_event(subscriber)?;

        let server = server::build(
            server,
            &self.application_protocols,
            &self.tls,
            &self.congestion_controller,
        )?;

        eprintln!("Server listening on port {}", self.io.port);

        Ok(server)
    }
}

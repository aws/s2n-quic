// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{perf, tls, Result};
use futures::future::try_join_all;
use s2n_quic::{
    stream::{BidirectionalStream, ReceiveStream, SendStream},
    Connection, Server,
};
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::spawn;

#[derive(Debug, StructOpt)]
pub struct Perf {
    #[structopt(long)]
    certificate: Option<PathBuf>,

    #[structopt(long)]
    private_key: Option<PathBuf>,

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    application_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(flatten)]
    limits: perf::Limits,

    /// Logs statistics for the endpoint
    #[structopt(long)]
    stats: bool,

    #[structopt(flatten)]
    io: crate::io::Server,
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
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

        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(
                tls::default::ca(self.certificate.as_ref())?,
                tls::default::private_key(self.private_key.as_ref())?,
            )?
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

        let server = Server::builder()
            .with_limits(self.limits.limits())?
            .with_io(io)?
            .with_event(subscriber)?
            .with_tls(tls)?
            .start()
            .unwrap();

        eprintln!("Server listening on port {}", self.io.port);

        Ok(server)
    }
}

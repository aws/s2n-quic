// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{perf, Result};
use futures::future::try_join_all;
use s2n_quic::{
    provider::{
        event, io,
        tls::default::certificate::{Certificate, IntoCertificate, IntoPrivateKey, PrivateKey},
    },
    stream::{BidirectionalStream, ReceiveStream, SendStream},
    Connection, Server,
};
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::spawn;

#[derive(Debug, StructOpt)]
pub struct Perf {
    #[structopt(short, long, default_value = "::")]
    ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(long)]
    certificate: Option<PathBuf>,

    #[structopt(long)]
    private_key: Option<PathBuf>,

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    alpn_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(long)]
    disable_gso: bool,
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

            println!("closing server after {} connections", limit);

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
                                //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.3.1
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
                                //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.3.1
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

        //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.3.2
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

        //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.3.2
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
        let private_key = self.private_key()?;
        let certificate = self.certificate()?;

        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(certificate, private_key)?
            .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
            .with_key_logging()?
            .build()?;

        let mut io_builder =
            io::Default::builder().with_receive_address((self.ip, self.port).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let server = Server::builder()
            .with_io(io)?
            .with_tls(tls)?
            .with_event(event::disabled::Provider)?
            .start()
            .unwrap();

        eprintln!("Server listening on port {}", self.port);

        Ok(server)
    }

    fn certificate(&self) -> Result<Certificate> {
        Ok(if let Some(pathbuf) = self.certificate.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }

    fn private_key(&self) -> Result<PrivateKey> {
        Ok(if let Some(pathbuf) = self.private_key.as_ref() {
            pathbuf.into_private_key()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.into_private_key()?
        })
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::Bytes;
use futures::future::try_join_all;
use s2n_quic::{
    provider::{
        event,
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
            let (size, _prelude) = read_stream_size(&mut receiver).await?;

            let receiver = tokio::spawn(async move { handle_receive_stream(receiver).await });
            let sender = tokio::spawn(async move { handle_send_stream(sender, size).await });

            let _ = futures::try_join!(receiver, sender);

            Ok(())
        }

        //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.3.2
        //# When a client uses a unidirectional stream to request a response
        //# payload from the server, the server opens a new unidirectional stream
        //# to send the requested data.  If no data is requested by the client,
        //# the server need take no action.
        async fn handle_uni_stream(mut receiver: ReceiveStream, sender: SendStream) -> Result<()> {
            let (size, _prelude) = read_stream_size(&mut receiver).await?;

            let receiver = tokio::spawn(async move { handle_receive_stream(receiver).await });
            let sender = tokio::spawn(async move { handle_send_stream(sender, size).await });

            let _ = futures::try_join!(receiver, sender);

            Ok(())
        }

        async fn handle_receive_stream(mut stream: ReceiveStream) -> Result<()> {
            let mut chunks = vec![Bytes::new(); 64];

            loop {
                let (len, is_open) = stream.receive_vectored(&mut chunks).await?;

                if !is_open {
                    break;
                }

                for chunk in chunks[..len].iter_mut() {
                    // discard chunks
                    *chunk = Bytes::new();
                }
            }

            Ok(())
        }

        async fn handle_send_stream(mut stream: SendStream, len: u64) -> Result<()> {
            let mut chunks = vec![Bytes::new(); 64];

            //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#4.1
            //# Since the goal here is to measure the efficiency of the QUIC
            //# implementation and not any application protocol, the performance
            //# application layer should be as light-weight as possible.  To this
            //# end, the client and server application layer may use a single
            //# preallocated and initialized buffer that it queues to send when any
            //# payload needs to be sent out.
            let mut data = s2n_quic_core::stream::testing::Data::new(len);

            loop {
                match data.send(usize::MAX, &mut chunks) {
                    Some(count) => {
                        stream.send_vectored(&mut chunks[..count]).await?;
                    }
                    None => {
                        stream.finish()?;
                        break;
                    }
                }
            }

            Ok(())
        }

        //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.3.1
        //# Every stream opened by the client uses the first 8 bytes of the
        //# stream data to encode a 64-bit unsigned integer in network byte order
        //# to indicate the length of data the client wishes the server to
        //# respond with.
        async fn read_stream_size(stream: &mut ReceiveStream) -> Result<(u64, Bytes)> {
            let mut chunk = Bytes::new();
            let mut offset = 0;
            let mut id = [0u8; core::mem::size_of::<u64>()];

            while offset < id.len() {
                chunk = stream
                    .receive()
                    .await?
                    .expect("every stream should be prefixed with the scenario ID");

                let needed_len = id.len() - offset;
                let len = chunk.len().min(needed_len);

                id[offset..offset + len].copy_from_slice(&chunk[..len]);
                offset += len;
                bytes::Buf::advance(&mut chunk, len);
            }

            let id = u64::from_be_bytes(id);

            Ok((id, chunk))
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

        let server = Server::builder()
            .with_io((self.ip, self.port))?
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

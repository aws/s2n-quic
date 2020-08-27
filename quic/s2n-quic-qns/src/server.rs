use crate::{
    endpoint::{AcceptExt, ConnectionExt, Endpoint, StreamExt},
    socket::Socket,
};
use bytes::Bytes;
use s2n_quic_core::{stream::StreamType, transport::parameters};
use s2n_quic_rustls as rustls;
use std::{io, path::PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Interop {
    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(long, default_value = "5000")]
    io_buffer_count: usize,

    #[structopt(long, default_value = "1500")]
    io_buffer_size: usize,

    #[structopt(long)]
    certificate: Option<PathBuf>,

    #[structopt(long)]
    private_key: Option<PathBuf>,

    #[structopt(long, default_value = "hq-29")]
    alpn_protocols: Vec<String>,
}

impl Interop {
    pub async fn run(&self) -> io::Result<()> {
        let endpoint = self.endpoint()?;

        let (listener, mut acceptor) = endpoint.listen();

        // the listener task will send/receive datagrams and notify connections of progress
        tokio::spawn(async move { listener.await.expect("Endpoint closed unexpectedly") });

        loop {
            let mut connection = acceptor.accept().await;
            println!("Accepted a QUIC connection!");

            // spawn a task per connection
            tokio::spawn(async move {
                while let Ok(mut stream) = connection.accept(StreamType::Bidirectional).await {
                    // spawn a task per stream
                    tokio::spawn(async move {
                        println!("Accepted a Stream");

                        loop {
                            let data = match stream.pop().await {
                                Ok(Some(data)) => data,
                                Ok(None) => {
                                    eprintln!("End of Stream");
                                    // Finish the response
                                    if let Err(e) = stream.finish().await {
                                        eprintln!("Stream error: {:?}", e);
                                    }
                                    return;
                                }
                                Err(e) => {
                                    eprintln!("Stream error: {:?}", e);
                                    return;
                                }
                            };

                            println!("Received {:?}", std::str::from_utf8(&data[..]));

                            // Send a response
                            let response = Bytes::from_static(b"HTTP/3 500 Work In Progress");
                            if let Err(e) = stream.push(response).await {
                                eprintln!("Stream error: {:?}", e);
                                return;
                            }
                            // TODO: This should actually not be here. We would only close the
                            // Stream if the peer closed their stream before.
                            // However in the current state the peer can't close the Stream, since we
                            // do not send an ACK for this yet. Therefore remove this once ACKs are sent.
                            if let Err(e) = stream.finish().await {
                                eprintln!("Stream error: {:?}", e);
                            }
                        }
                    });
                }
            });
        }
    }

    fn bind(&self) -> Result<Socket, io::Error> {
        self.check_testcase();
        let socket = Socket::bind(
            ("0.0.0.0", self.port),
            self.io_buffer_count,
            self.io_buffer_size,
        )?;
        println!("Server listening on port {:?}", self.port);
        Ok(socket)
    }

    fn endpoint(&self) -> Result<Endpoint, io::Error> {
        let socket = self.bind()?;
        Ok(Endpoint::new(
            socket,
            create_rustls_server(
                self.certificate()?,
                self.private_key()?,
                &self.alpn_protocols,
            ),
            create_server_params(),
        ))
    }

    fn certificate(&self) -> Result<Vec<u8>, io::Error> {
        if let Some(path) = self.certificate.as_ref() {
            std::fs::read(path)
        } else {
            Ok(CERTIFICATE.to_vec())
        }
    }

    fn private_key(&self) -> Result<Vec<u8>, io::Error> {
        if let Some(path) = self.private_key.as_ref() {
            std::fs::read(path)
        } else {
            Ok(PRIVATE_KEY.to_vec())
        }
    }

    fn check_testcase(&self) {
        match std::env::var("TESTCASE").ok().as_deref() {
            // TODO uncomment once connection id authentication is done
            // Some("handshake") | Some("transfer") => {}
            None => {
                eprintln!("missing TESTCASE environment variable");
                std::process::exit(127);
            }
            _ => {
                eprintln!("unsupported");
                std::process::exit(127);
            }
        }
    }
}

const CERTIFICATE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/cert.der"));
const PRIVATE_KEY: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/key.der"));

/// The transport parameters we are using for our QUIC endpoint
fn create_server_params() -> parameters::ServerTransportParameters {
    parameters::ServerTransportParameters::default()
}

/// Create the TLS configuration we are using for the QUIC endpoint
fn create_rustls_server(
    certificate: Vec<u8>,
    private_key: Vec<u8>,
    alpn_protocols: &[String],
) -> rustls::Server {
    rustls::Server::builder()
        .with_certificate(&certificate, &private_key)
        .expect("invalid certificate")
        .with_alpn_protocols(alpn_protocols.iter().map(String::as_bytes))
        .expect("invalid alpn")
        .build()
        .expect("invalid server")
}

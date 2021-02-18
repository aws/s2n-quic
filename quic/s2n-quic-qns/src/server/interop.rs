use crate::{file::File, Result};
use bytes::Bytes;
use core::convert::TryInto;
use futures::stream::StreamExt;
use s2n_quic::{
    provider::{
        endpoint_limits,
        tls::default::certificate::{Certificate, IntoCertificate, IntoPrivateKey, PrivateKey},
    },
    stream::BidirectionalStream,
    Connection, Server,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use structopt::StructOpt;
use tokio::spawn;

#[derive(Debug, StructOpt)]
pub struct Interop {
    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(long)]
    certificate: Option<PathBuf>,

    #[structopt(long)]
    private_key: Option<PathBuf>,

    #[structopt(long, default_value = "hq-29")]
    alpn_protocols: Vec<String>,

    #[structopt(long, default_value = ".")]
    www_dir: PathBuf,
}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        self.check_testcase();

        let mut server = self.server()?;

        let www_dir: Arc<Path> = Arc::from(self.www_dir.as_path());

        while let Some(connection) = server.accept().await {
            println!("Accepted a QUIC connection!");

            // TODO check the ALPN of the connection to determine handler

            // spawn a task per connection
            spawn(handle_h09_connection(connection, www_dir.clone()));
        }

        async fn handle_h09_connection(mut connection: Connection, www_dir: Arc<Path>) {
            loop {
                match connection.accept_bidirectional_stream().await {
                    Ok(Some(stream)) => {
                        let www_dir = www_dir.clone();
                        // spawn a task per stream
                        tokio::spawn(async move {
                            if let Err(err) = handle_h09_stream(stream, www_dir).await {
                                eprintln!("Stream errror: {:?}", err)
                            }
                        });
                    }
                    Ok(None) => {
                        // the connection was closed without an error
                        return;
                    }
                    Err(err) => {
                        eprintln!("error while accepting stream: {}", err);
                        return;
                    }
                }
            }
        }

        async fn handle_h09_stream(
            mut stream: BidirectionalStream,
            www_dir: Arc<Path>,
        ) -> Result<()> {
            let path = handle_h09_request(&mut stream).await?;
            let mut abs_path = www_dir.to_path_buf();
            abs_path.extend(
                path.split('/')
                    .filter(|segment| !segment.starts_with('.'))
                    .map(std::path::Path::new),
            );
            let mut file = File::open(&abs_path).await?;
            loop {
                match file.next().await {
                    Some(Ok(chunk)) => stream.send(chunk).await?,
                    Some(Err(err)) => {
                        stream.reset(1u32.try_into()?)?;
                        return Err(err.into());
                    }
                    None => {
                        stream.finish()?;
                        return Ok(());
                    }
                }
            }
        }

        async fn handle_h09_request(stream: &mut BidirectionalStream) -> Result<String> {
            let mut path = String::new();
            let mut chunks = vec![Bytes::new(), Bytes::new()];
            let mut total_chunks = 0;
            loop {
                // grow the chunks
                if chunks.len() == total_chunks {
                    chunks.push(Bytes::new());
                }
                let (consumed, is_open) =
                    stream.receive_vectored(&mut chunks[total_chunks..]).await?;
                total_chunks += consumed;
                if parse_h09_request(&chunks[..total_chunks], &mut path, is_open)? {
                    return Ok(path);
                }
            }
        }

        fn parse_h09_request(chunks: &[Bytes], path: &mut String, is_open: bool) -> Result<bool> {
            let mut bytes = chunks.iter().flat_map(|chunk| chunk.iter().cloned());

            macro_rules! expect {
                ($char:literal) => {
                    match bytes.next() {
                        Some($char) => {}
                        None if is_open => return Ok(false),
                        _ => return Err("invalid request".into()),
                    }
                };
            }

            expect!(b'G');
            expect!(b'E');
            expect!(b'T');
            expect!(b' ');
            expect!(b'/');

            loop {
                match bytes.next() {
                    Some(c @ b'0'..=b'9') => path.push(c as char),
                    Some(c @ b'a'..=b'z') => path.push(c as char),
                    Some(c @ b'A'..=b'Z') => path.push(c as char),
                    Some(b'.') => path.push('.'),
                    Some(b'/') => path.push('/'),
                    Some(b'-') => path.push('-'),
                    Some(b'\n') | Some(b'\r') => return Ok(true),
                    Some(c) => return Err(format!("invalid request {}", c as char).into()),
                    None => return Ok(!is_open),
                }
            }
        }

        Ok(())
    }

    fn server(&self) -> Result<Server> {
        let private_key = self.private_key()?;
        let certificate = self.certificate()?;

        // The server builder defaults to a chain because this allows certs to just work, whether
        // the PEM contains a single cert or a chain
        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(certificate, private_key)?
            .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
            .with_key_logging()?
            .build()?;

        let mut max_handshakes = 100;
        if let Some("retry") = std::env::var("TESTCASE").ok().as_deref() {
            max_handshakes = 0;
        }

        let limits = endpoint_limits::Default::builder()
            .with_inflight_handshake_limit(max_handshakes)?
            .build()?;

        let server = Server::builder()
            .with_io(("::", self.port))?
            .with_tls(tls)?
            .with_endpoint_limits(limits)?
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

    fn check_testcase(&self) {
        let is_supported = match std::env::var("TESTCASE").ok().as_deref() {
            Some("versionnegotiation") => false,
            Some("handshake") => true,
            Some("transfer") => true,
            Some("chacha20") => true,
            Some("retry") => true,
            Some("resumption") => false,
            Some("zerortt") => false,
            Some("http3") => false,
            Some("multiconnect") => true,
            Some("handshakecorruption") => true,
            Some("transfercorruption") => true,
            Some("ecn") => false,
            Some("rebind-addr") => false,
            Some("crosstraffic") => true,
            Some("keyupdate") => true,
            None => true,
            _ => false,
        };

        if !is_supported {
            eprintln!("unsupported");
            std::process::exit(127);
        }
    }
}

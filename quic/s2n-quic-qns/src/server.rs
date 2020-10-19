use crate::Result;
use bytes::Bytes;
use s2n_quic::{stream::BidirectionalStream, Connection, Server};
use std::path::PathBuf;
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
}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        self.check_testcase();

        let mut server = self.server()?;

        while let Some(connection) = server.accept().await {
            println!("Accepted a QUIC connection!");

            // spawn a task per connection
            spawn(handle_connection(connection));
        }

        async fn handle_connection(mut connection: Connection) {
            while let Ok(stream) = connection.accept_bidirectional_stream().await {
                // spawn a task per stream
                tokio::spawn(async move {
                    println!("Accepted a Stream");

                    if let Err(err) = handle_stream(stream).await {
                        eprintln!("Stream errror: {:?}", err)
                    }
                });
            }
        }

        async fn handle_stream(mut stream: BidirectionalStream) -> Result<()> {
            loop {
                let data = match stream.pop().await? {
                    Some(data) => data,
                    None => {
                        eprintln!("End of Stream");
                        // Finish the response
                        stream.finish().await?;
                        return Ok(());
                    }
                };

                println!("Received {:?}", std::str::from_utf8(&data[..]));

                // Send a response
                let response = Bytes::from_static(b"HTTP/3 500 Work In Progress");
                stream.push(response).await?;
            }
        }

        Ok(())
    }

    fn server(&self) -> Result<Server> {
        let certificate = self.certificate()?;
        let private_key = self.private_key()?;

        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(&certificate, &private_key)?
            .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
            .build()?;

        let server = Server::builder()
            .with_io(("0.0.0.0", self.port))?
            .with_tls(tls)?
            .start()?;

        eprintln!("Server listening on port {}", self.port);

        Ok(server)
    }

    fn certificate(&self) -> Result<Vec<u8>> {
        Ok(if let Some(path) = self.certificate.as_ref() {
            std::fs::read(path)?
        } else {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/cert.der")).to_vec()
        })
    }

    fn private_key(&self) -> Result<Vec<u8>> {
        Ok(if let Some(path) = self.private_key.as_ref() {
            std::fs::read(path)?
        } else {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/key.der")).to_vec()
        })
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

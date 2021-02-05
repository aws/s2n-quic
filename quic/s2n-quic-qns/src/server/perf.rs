use crate::Result;
use bytes::Bytes;
use s2n_quic::{
    provider::tls::default::{AsCertificate, AsPrivateKey, Certificate, PrivateKey},
    stream::{BidirectionalStream, ReceiveStream, SendStream},
    Connection, Server,
};
use std::path::PathBuf;
use structopt::StructOpt;
use tokio::spawn;

#[derive(Debug, StructOpt)]
pub struct Perf {
    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(long)]
    certificate: Option<PathBuf>,

    #[structopt(long)]
    private_key: Option<PathBuf>,

    #[structopt(long, default_value = "perf")]
    alpn_protocols: Vec<String>,
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        let mut server = self.server()?;

        while let Some(connection) = server.accept().await {
            // spawn a task per connection
            spawn(handle_connection(connection));
        }

        async fn handle_connection(connection: Connection) {
            let (_handle, acceptor) = connection.split();
            let (mut bidi, mut uni) = acceptor.split();

            macro_rules! accept {
                ($stream:expr, $onstream:ident) => {
                    tokio::spawn(async move {
                        loop {
                            match $stream.await? {
                                Some(stream) => {
                                    // spawn a task per stream
                                    tokio::spawn(async move {
                                        // ignore stream errors
                                        let _ = $onstream(stream).await;
                                    });
                                }
                                None => {
                                    // the connection was closed without an error
                                    return <Result<()>>::Ok(());
                                }
                            }
                        }
                    })
                };
            }

            let bidi = accept!(bidi.accept_bidirectional_stream(), handle_bidi_stream);
            let uni = accept!(uni.accept_receive_stream(), handle_receive_stream);

            let _ = futures::try_join!(bidi, uni);
        }

        async fn handle_bidi_stream(stream: BidirectionalStream) -> Result<()> {
            let (mut receiver, sender) = stream.split();
            let (size, _prelude) = read_stream_size(&mut receiver).await?;

            let receiver = tokio::spawn(async move { handle_receive_stream(receiver).await });
            let sender = tokio::spawn(async move { handle_send_stream(sender, size).await });

            let _ = futures::try_join!(receiver, sender);

            Ok(())
        }

        async fn handle_receive_stream(mut stream: ReceiveStream) -> Result<()> {
            let mut chunks = vec![Bytes::new(); 16];

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
            let mut chunks = vec![Bytes::new(); 16];
            let mut data = s2n_quic_integration::stream::Data::new(len as usize);

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

        Ok(())
    }

    fn server(&self) -> Result<Server> {
        let private_key = self.private_key()?;
        let certificate = self.certificate()?;

        let tls = s2n_quic::provider::tls::default::Server::builder()
            .with_certificate(certificate, private_key)?
            .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
            .build()?;

        let server = Server::builder()
            .with_io(("::", self.port))?
            .with_tls(tls)?
            .start()
            .unwrap();

        eprintln!("Server listening on port {}", self.port);

        Ok(server)
    }

    fn certificate(&self) -> Result<Vec<Certificate>> {
        Ok(if let Some(pathbuf) = self.certificate.as_ref() {
            pathbuf.as_certificate()?
        } else {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/cert.der"))
                .as_certificate()?
        })
    }

    fn private_key(&self) -> Result<PrivateKey> {
        Ok(if let Some(pathbuf) = self.private_key.as_ref() {
            pathbuf.as_private_key()?
        } else {
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/certs/key.der"))
                .as_private_key()?
        })
    }
}

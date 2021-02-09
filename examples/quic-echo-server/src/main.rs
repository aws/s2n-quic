use s2n_quic::Server;
use std::error::Error;

const CERT: &str = include_str!("../../../quic/s2n-quic-qns/certs/cert.pem");
const KEY: &str = include_str!("../../../quic/s2n-quic-qns/certs/key.pem");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut server = Server::builder()
        .with_tls((CERT, KEY))?
        .with_io("127.0.0.1:4433")?
        .start()?;

    while let Some(mut connection) = server.accept().await {
        // spawn a new task for the connection
        tokio::spawn(async move {
            eprintln!("Connection accepted from {:?}", connection.remote_addr());

            while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                // spawn a new task for the stream
                tokio::spawn(async move {
                    eprintln!("Stream opened from {:?}", stream.connection().remote_addr());

                    // echo any data back to the stream
                    while let Ok(Some(data)) = stream.receive().await {
                        stream.send(data).await.expect("stream should be open");
                    }
                });
            }
        });
    }

    Ok(())
}

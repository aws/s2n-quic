# s2n-quic

s2n-quic is a Rust implementation of the [QUIC protocol](https://quicwg.org/)

[![Crates.io][crates-badge]][crates-url]
[![docs.rs][docs-badge]][docs-url]
[![Apache 2.0 Licensed][license-badge]][license-url]
[![Build Status][actions-badge]][actions-url]
[![Dependencies][dependencies-badge]][dependencies-url]
[![MSRV][msrv-badge]][msrv-url]

## Installation

`s2n-quic` is available on `crates.io` and can be added to a project like so:

```toml
[dependencies]
s2n-quic = "1"
```

__NOTE__: On unix systems, [s2n-tls](https://github.com/aws/s2n-tls) will be used as the default TLS provider and requires a C compiler to be installed.

## Example

The following implements a basic echo server and client. The client connects to the server and pipes its `stdin` on a stream. The server listens for new streams and pipes any data it receives back to the client. The client will then pipe all stream data to `stdout`.

### Server

```rust
// src/bin/server.rs
use s2n_quic::Server;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut server = Server::builder()
        .with_tls(("./path/to/cert.pem", "./path/to/key.pem"))?
        .with_io("127.0.0.1:4433")?
        .start()?;

    while let Some(mut connection) = server.accept().await {
        // spawn a new task for the connection
        tokio::spawn(async move {
            while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                // spawn a new task for the stream
                tokio::spawn(async move {
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
```

### Client

```rust
// src/bin/client.rs
use s2n_quic::{client::Connect, Client};
use std::{error::Error, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .with_tls(CERT_PEM)?
        .with_io("0.0.0.0:0")?
        .start()?;

    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await?;

    // ensure the connection doesn't time out with inactivity
    connection.keep_alive(true)?;

    // open a new stream and split the receiving and sending sides
    let stream = connection.open_bidirectional_stream().await?;
    let (mut receive_stream, mut send_stream) = stream.split();

    // spawn a task that copies responses from the server to stdout
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let _ = tokio::io::copy(&mut receive_stream, &mut stdout).await;
    });

    // copy data from stdin and send it to the server
    let mut stdin = tokio::io::stdin();
    tokio::io::copy(&mut stdin, &mut send_stream).await?;

    Ok(())
}
```

### MSRV

s2n-quic will maintain a rolling MSRV (minimum supported rust version) policy of at least 6 months.

The current MSRV is [1.53.0][msrv-url].

## Security

See [CONTRIBUTING](CONTRIBUTING.md#security-issue-notifications) for more information.

## License

This project is licensed under the [Apache-2.0 License][license-url].

[crates-badge]: https://img.shields.io/crates/v/s2n-quic.svg
[crates-url]: https://crates.io/crates/s2n-quic
[license-badge]: https://img.shields.io/badge/license-apache-blue.svg
[license-url]: https://aws.amazon.com/apache-2-0/
[actions-badge]: https://github.com/awslabs/s2n-quic/workflows/ci/badge.svg
[actions-url]: https://github.com/awslabs/s2n-quic/actions/workflows/ci.yml?query=branch%3Amain
[docs-badge]: https://img.shields.io/docsrs/s2n-quic.svg
[docs-url]: https://docs.rs/s2n-quic
[dependencies-badge]: https://img.shields.io/librariesio/release/cargo/s2n-quic.svg
[dependencies-url]: https://crates.io/crates/s2n-quic/dependencies
[msrv-badge]: https://img.shields.io/badge/MSRV-1.53.0-green
[msrv-url]: https://blog.rust-lang.org/2021/06/17/Rust-1.53.0.html

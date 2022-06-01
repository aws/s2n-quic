# s2n-quic

`s2n-quic` is a Rust implementation of the [IETF QUIC protocol](https://quicwg.org/), featuring:
* a simple, easy-to-use API. See [an example](https://github.com/aws/s2n-quic/blob/main/examples/echo/src/bin/quic_echo_server.rs) of an s2n-quic echo server built with just a few API calls
* high configurability using [providers](https://docs.rs/s2n-quic/latest/s2n_quic/provider/index.html) for granular control of functionality
* extensive automated testing, including fuzz testing, integration testing, unit testing, snapshot testing, efficiency testing, performance benchmarking, interopability testing and [more](https://github.com/aws/s2n-quic/blob/main/docs/ci.md)
* integration with [s2n-tls](https://github.com/aws/s2n-tls), AWS's simple, small, fast and secure TLS implementation, as well as [rustls](https://crates.io/crates/rustls)
* thorough [compliance coverage tracking](https://github.com/aws/s2n-quic/blob/main/docs/ci.md#compliance) of normative language in relevant standards
* and much more, including [CUBIC congestion controller](https://www.rfc-editor.org/rfc/rfc8312.html) support, [packet pacing](https://www.rfc-editor.org/rfc/rfc9002.html#name-pacing), [Generic Segmentation Offload](https://lwn.net/Articles/188489/) support, [Path MTU discovery](https://www.rfc-editor.org/rfc/rfc8899.html), and unique [connection identifiers](https://www.rfc-editor.org/rfc/rfc9000.html#name-connection-id) detached from the address

See the [API documentation](https://docs.rs/s2n-quic) and [examples](https://github.com/aws/s2n-quic/tree/main/examples) to get started with `s2n-quic`.

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

__NOTE__: On unix-like systems, [`s2n-tls`](https://github.com/aws/s2n-tls) will be used as the default TLS provider and requires a C compiler to be installed.

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

## Minimum Supported Rust Version (MSRV)

`s2n-quic` will maintain a rolling MSRV (minimum supported rust version) policy of at least 6 months. The current s2n-quic version is not guaranteed to build on Rust versions earlier than the MSRV.

The current MSRV is [1.56.1][msrv-url].

## Security issue notifications
If you discover a potential security issue in s2n-quic we ask that you notify
AWS Security via our [vulnerability reporting page](http://aws.amazon.com/security/vulnerability-reporting/). Please do **not** create a public github issue.

If you package or distribute s2n-quic, or use s2n-quic as part of a large multi-user service, you may be eligible for pre-notification of future s2n-quic releases. Please contact s2n-pre-notification@amazon.com.

## License

This project is licensed under the [Apache-2.0 License][license-url].

[crates-badge]: https://img.shields.io/crates/v/s2n-quic.svg
[crates-url]: https://crates.io/crates/s2n-quic
[license-badge]: https://img.shields.io/badge/license-apache-blue.svg
[license-url]: https://aws.amazon.com/apache-2-0/
[actions-badge]: https://github.com/aws/s2n-quic/workflows/ci/badge.svg
[actions-url]: https://github.com/aws/s2n-quic/actions/workflows/ci.yml?query=branch%3Amain
[docs-badge]: https://img.shields.io/docsrs/s2n-quic.svg
[docs-url]: https://docs.rs/s2n-quic
[dependencies-badge]: https://img.shields.io/librariesio/release/cargo/s2n-quic.svg
[dependencies-url]: https://crates.io/crates/s2n-quic/dependencies
[msrv-badge]: https://img.shields.io/badge/MSRV-1.56.1-green
[msrv-url]: https://blog.rust-lang.org/2021/11/01/Rust-1.56.1.html

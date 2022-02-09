# s2n-quic-qns

This crate contains an application for testing the s2n-quic client and server modes with various application-level protocols. The current protocols include:

* [`hq-interop`](https://github.com/quicwg/base-drafts/wiki/21st-Implementation-Draft#overview)
* [`perf`](https://tools.ietf.org/id/draft-banks-quic-performance-00.html)

## Building

Ensure the build requirement are met in the main `s2n-quic` readme. Then run:

```bash
cargo build --release --bin s2n-quic-qns
```

The `s2n-quic-qns` application will be available at `target/release/s2n-quic-qns`.

## Usage

### hq-interop

This application protocol is designed for executing tests defined in the [`quic-interop-runner`](https://github.com/marten-seemann/quic-interop-runner). The server serves a directory (defaulting to `.`). The client connects to the server and can request the served files by opening a stream and issuing a [`HTTP 0.9` request](https://www.w3.org/Protocols/HTTP/AsImplemented.html). The server responds with the contents of the file and closes the stream.

#### Examples

__Server__:

```bash
# start the interop server on port 4433
./target/release/s2n-quic-qns interop server --port 4433
```

__Client__:

```bash
# downloads a single file and prints it to stdout
./target/release/s2n-quic-qns interop client https://localhost:4433/Cargo.toml
```

```bash
# multiple requests can be downloaded to a directory
./target/release/s2n-quic-qns interop client --download-dir files https://localhost:4433/Cargo.toml https://localhost:4433/README.md
```

### perf

This application protocol is designed for testing throughput and efficiency of quic implementations. The client opens one or more connections to a server and opens one or more streams, which include the number of bytes that should be transmitted.

__Server__:

```bash
# start the perf server on port 4433
./target/release/s2n-quic-qns perf server --port 4433
```

__Client__:


**NOTE** the client is currently not implemented and a 3rd party client is currently needed to connect to the server

```bash
# open a perf connection
./target/release/s2n-quic-qns perf client --host localhost:4433 1Mb-up-2Mb-down wait-1s 10Mb-down
```

## License

This project is licensed under the [Apache-2.0 License][license-url].

[license-badge]: https://img.shields.io/badge/license-apache-blue.svg
[license-url]: https://aws.amazon.com/apache-2-0/

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::Criterion;
use s2n_quic_dc::stream::{self, server::accept, socket::Protocol};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const ITERATIONS: u64 = 120;
static BUFFER: &[u8] = &[0x0; 1024 * 1024];

async fn copy_data(a: impl AsyncWrite + Send + 'static, b: impl AsyncRead + Send + 'static) {
    let a = tokio::spawn(async move {
        tokio::pin!(a);
        for _ in 0..ITERATIONS {
            a.write_all(BUFFER).await.unwrap();
        }
        a.shutdown().await.unwrap();
    });

    let b = tokio::spawn(async move {
        tokio::pin!(b);
        let mut void = vec![0; BUFFER.len()];
        while b.read(&mut void[..]).await.unwrap() != 0 {
            // Read until EOF
        }
    });

    tokio::try_join!(a, b).unwrap();
}

fn pair(
    protocol: Protocol,
    accept_flavor: accept::Flavor,
) -> (stream::testing::Client, stream::testing::Server) {
    let client = stream::testing::Client::default();
    let server = stream::testing::Server::builder()
        .protocol(protocol)
        .accept_flavor(accept_flavor)
        .build();
    client.handshake_with(&server).unwrap();
    (client, server)
}

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("streams/throughput");

    group.throughput(criterion::Throughput::Bytes(
        ITERATIONS * BUFFER.len() as u64,
    ));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    group.bench_function(criterion::BenchmarkId::new("duplex", ""), |b| {
        b.to_async(&rt).iter(move || async move {
            let (a, b) = tokio::io::duplex(1024 * 1024);
            copy_data(a, b).await;
        });
    });

    group.bench_function(criterion::BenchmarkId::new("tcp", ""), |b| {
        b.to_async(&rt).iter(move || async move {
            let server = TcpListener::bind("localhost:0").await.unwrap();
            let server_addr = server.local_addr().unwrap();
            let (a, b) = tokio::join!(TcpStream::connect(server_addr), async move {
                server.accept().await.unwrap().0
            });
            copy_data(a.unwrap(), b).await;
        });
    });

    for protocol in [Protocol::Udp, Protocol::Tcp] {
        let _rt = rt.enter();
        let (client, server) = pair(protocol, accept::Flavor::Fifo);
        let name = format!("{protocol:?}").to_lowercase();
        group.bench_function(criterion::BenchmarkId::new("dcquic", name), |b| {
            b.to_async(&rt).iter(|| {
                let client = &client;
                let server = &server;
                async move {
                    let (a, b) =
                        tokio::join!(async { client.connect_to(server).await.unwrap() }, async {
                            let (b, _addr) = server.accept().await.unwrap();
                            b
                        });

                    copy_data(a, b).await;
                }
            });
        });
    }
}

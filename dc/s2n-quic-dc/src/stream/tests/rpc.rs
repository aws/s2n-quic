// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{
        client::rpc::{self, InMemoryResponse},
        testing::{dcquic::Context, Client, Server},
        Protocol,
    },
    testing::{ext::*, server_name, sim, without_tracing},
};
use bolero::{check, TypeGenerator};
use bytes::BytesMut;
use s2n_quic_core::stream::testing::Data;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info_span, Instrument};

fn hello_goodbye() {
    async move {
        let client = Client::builder().build();
        let response = rpc::InMemoryResponse::from(BytesMut::default());
        let response = client
            .rpc_sim("server:443", &b"hello!"[..], response)
            .await
            .unwrap();

        assert_eq!(response, b"goodbye!"[..]);
    }
    .group("client")
    .instrument(info_span!("client"))
    .primary()
    .spawn();

    async move {
        let server = Server::udp().port(443).build();

        while let Ok((mut stream, peer_addr)) = server.accept().await {
            async move {
                let mut request = vec![];
                stream.read_to_end(&mut request).await.unwrap();

                stream.write_from_fin(&mut &b"goodbye!"[..]).await.unwrap();
            }
            .instrument(info_span!("stream", ?peer_addr))
            .primary()
            .spawn();
        }
    }
    .group("server")
    .instrument(info_span!("server"))
    .spawn();
}

#[test]
fn simple() {
    sim(hello_goodbye);
}

// TODO use this with bach >= 0.0.13
#[cfg(todo)]
#[test]
fn no_loss() {
    use core::sync::atomic::{AtomicUsize, Ordering};

    static COUNT: AtomicUsize = AtomicUsize::new(0);

    sim(|| {
        hello_goodbye();

        ::bach::net::monitor::on_packet_sent(move |packet| {
            let count = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            assert!(
                count <= 4,
                "flow should only consume 4 packets\n{packet:#?}"
            );
            tracing::info!(?packet, "on_packet_sent");
            Default::default()
        });
    });

    assert_eq!(COUNT.load(Ordering::Relaxed), 4);
}

// TODO use this with bach >= 0.0.13
#[cfg(todo)]
#[test]
fn packet_loss() {
    use core::sync::atomic::{AtomicUsize, Ordering};

    check!()
        .exhaustive()
        .with_generator(0usize..=4)
        .cloned()
        .for_each(|loss_idx| {
            let max_count = match loss_idx {
                // the first two are Stream packets
                0..=1 => 6,
                // the next ones are Control packets, which cause 1 extra packet, since the
                // sender also needs to transmit the Stream packet again.
                2..=3 => 7,
                // otherwise, it should only take 4
                _ => 4,
            };

            static COUNT: AtomicUsize = AtomicUsize::new(0);

            // reset the count back to 0
            COUNT.store(0, Ordering::Relaxed);

            sim(|| {
                hello_goodbye();

                ::bach::net::monitor::on_packet_sent(move |packet| {
                    let idx = COUNT.fetch_add(1, Ordering::Relaxed);
                    let count = idx + 1;

                    assert!(
                        count <= max_count,
                        "flow should only consume {max_count} packets\n{packet:#?}"
                    );

                    if loss_idx == idx {
                        return ::bach::net::monitor::Command::Drop;
                    }

                    Default::default()
                });
            });

            assert_eq!(COUNT.swap(0, Ordering::Relaxed), max_count);
        });
}

#[test]
fn echo_stream() {
    without_tracing(|| {
        check!().with_test_time(30.s()).run(|| {
            sim(|| {
                async move {
                    let client = Client::builder().build();
                    let data = Data::new((0..=512_000).any());
                    let response = rpc::InMemoryResponse::from(data);
                    let response = client.rpc_sim("server:443", data, response).await.unwrap();

                    assert!(response.is_finished());
                }
                .group("client")
                .primary()
                .spawn();

                async move {
                    let server = Server::udp().port(443).build();

                    while let Ok((mut stream, _addr)) = server.accept().await {
                        async move {
                            let mut buffer = vec![];
                            // echo the response back
                            loop {
                                let len = stream.read_buf(&mut buffer).await.unwrap();
                                if len == 0 {
                                    break;
                                }

                                stream.write_all(&buffer[..len]).await.unwrap();
                                buffer.clear();
                            }
                        }
                        .spawn();
                    }
                }
                .group("server")
                .spawn();
            })
        })
    });
}

const MAX_LEN: u64 = 512_000;

#[derive(Clone, Copy, Debug, TypeGenerator)]
struct Harness {
    #[generator(1..=64)]
    num_clients: usize,
    #[generator(1..=64)]
    num_requests: usize,
    #[generator(0..=MAX_LEN)]
    req_size: u64,
    #[generator(0..=MAX_LEN)]
    res_size: u64,
    server_pause: u16,
    server_include_fin: bool,
}

impl Harness {
    fn run(self) {
        eprintln!("{self:?}");
        let Harness {
            num_clients,
            num_requests,
            req_size,
            res_size,
            server_pause,
            server_include_fin,
        } = self;

        for client in 0..num_clients {
            async move {
                (client as u64).us().sleep().await;

                let client = Client::builder().build();
                for _ in 0..num_requests {
                    let req = Data::new(req_size);
                    let response = rpc::InMemoryResponse::from(Data::new(res_size));
                    let response = client.rpc_sim("server:443", req, response).await.unwrap();

                    assert!(response.is_finished());
                }
            }
            .group("client")
            .instrument(info_span!("client", client))
            .primary()
            .spawn();
        }

        async move {
            let server = Server::udp()
                .port(443)
                .map_capacity(num_clients * 2)
                .build();

            while let Ok((mut stream, _addr)) = server.accept().await {
                async move {
                    let mut req = Data::new(req_size);
                    loop {
                        let Ok(len) = stream.read_into(&mut req).await else {
                            return;
                        };
                        if len == 0 {
                            break;
                        }
                    }

                    tracing::info!(?req, "received request");

                    (server_pause as u64).us().sleep().await;

                    let mut res = Data::new(res_size);

                    while !res.is_finished() {
                        if server_include_fin {
                            stream.write_from_fin(&mut res).await.unwrap();
                        } else {
                            stream.write_from(&mut res).await.unwrap();
                        }
                    }

                    tracing::info!(?res, "sent response");
                }
                .instrument(info_span!("stream"))
                .primary()
                .spawn();
            }
        }
        .group("server")
        .instrument(info_span!("server"))
        .spawn();
    }
}

#[test]
fn large_transfer() {
    sim(|| {
        // TODO use once bach 0.1 is released
        #[cfg(todo)]
        bach::net::monitor::on_packet_sent(|packet| {
            use bach::net::monitor::Command;

            // 25% chance of dropping a packet
            *bach::rand::pick(&[Command::Pass, Command::Pass, Command::Pass, Command::Drop])
        });

        Harness {
            num_clients: 1,
            num_requests: 1,
            req_size: 100_000_000,
            res_size: 10,
            server_pause: 1,
            server_include_fin: true,
        }
        .run();
    });
}

#[test]
fn fuzz_test() {
    without_tracing(|| {
        check!()
            .with_type::<Harness>()
            .cloned()
            .with_test_time(30.s())
            .for_each(|harness| sim(|| harness.run()))
    });
}

#[derive(Clone, Debug)]
struct RpcHarness {
    protocol: Protocol,
}

impl Default for RpcHarness {
    fn default() -> Self {
        Self {
            protocol: Protocol::Udp,
        }
    }
}

impl RpcHarness {
    async fn run(self) {
        let (client, server) = Context::new(self.protocol).await.split();
        let handshake_addr = server.handshake_addr().unwrap();
        let acceptor_addr = server.acceptor_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut stream, _peer_addr)) = server.accept().await {
                tokio::spawn(async move {
                    let mut buffer = Vec::with_capacity(1024);
                    while let Ok(n) = stream.read_buf(&mut buffer).await {
                        if n == 0 {
                            break;
                        }
                        stream.write_all(&buffer[..n]).await.unwrap();
                    }
                });
            }
        });

        let count = 1_000;

        for _ in 0..count {
            let request = &b"hello"[..];
            let response = InMemoryResponse::from(BytesMut::new());
            let response = client
                .rpc(
                    handshake_addr,
                    acceptor_addr,
                    request,
                    response,
                    server_name(),
                )
                .await
                .unwrap();
            assert_eq!(response.as_ref(), request);
        }
    }
}

macro_rules! tests {
    () => {
        #[tokio::test]
        async fn many_requests() {
            RpcHarness { ..rpc_harness() }.run().await
        }
    };
}

mod tcp {
    use super::*;

    fn rpc_harness() -> RpcHarness {
        RpcHarness {
            protocol: Protocol::Tcp,
        }
    }

    tests!();
}

mod udp {
    use super::*;

    fn rpc_harness() -> RpcHarness {
        RpcHarness {
            protocol: Protocol::Udp,
        }
    }

    tests!();
}

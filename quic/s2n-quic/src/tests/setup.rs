// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::Connect,
    provider::{
        event,
        io::testing::{primary, spawn, Handle, Result},
    },
    stream::PeerStream,
    Client, Server,
};
use rand::{Rng, RngCore};
use s2n_quic_core::{crypto::tls::testing::certificates, havoc, stream::testing::Data};
use std::net::SocketAddr;

pub static SERVER_CERTS: (&str, &str) = (certificates::CERT_PEM, certificates::KEY_PEM);

pub fn tracing_events() -> event::tracing::Subscriber {
    use std::sync::Once;

    static TRACING: Once = Once::new();

    // make sure this only gets initialized once
    TRACING.call_once(|| {
        let format = tracing_subscriber::fmt::format()
            .with_level(false) // don't include levels in formatted output
            .with_timer(Uptime)
            .with_ansi(false)
            .compact(); // Use a less verbose output format.

        struct Uptime;

        // Generate the timestamp from the testing IO provider rather than wall clock.
        impl tracing_subscriber::fmt::time::FormatTime for Uptime {
            fn format_time(
                &self,
                w: &mut tracing_subscriber::fmt::format::Writer<'_>,
            ) -> std::fmt::Result {
                write!(w, "{}", crate::provider::io::testing::now())
            }
        }

        let level = if std::env::var("TRACE").is_ok() {
            tracing_subscriber::filter::LevelFilter::TRACE
        } else {
            tracing_subscriber::filter::LevelFilter::DEBUG
        };

        tracing_subscriber::fmt()
            .with_max_level(level)
            .event_format(format)
            .with_test_writer()
            .init();
    });

    event::tracing::Subscriber::default()
}

pub fn start_server(mut server: Server) -> Result<SocketAddr> {
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            tracing::debug!("accepted server connection: {}", connection.id());
            spawn(async move {
                while let Ok(Some(stream)) = connection.accept().await {
                    tracing::debug!("accepted server stream: {}", stream.id());
                    match stream {
                        PeerStream::Receive(mut stream) => {
                            spawn(async move {
                                while let Ok(Some(_)) = stream.receive().await {
                                    // noop
                                }
                            });
                        }
                        PeerStream::Bidirectional(mut stream) => {
                            spawn(async move {
                                while let Ok(Some(chunk)) = stream.receive().await {
                                    let _ = stream.send(chunk).await;
                                }
                            });
                        }
                    }
                }
            });
        }
    });

    Ok(server_addr)
}

pub fn server(handle: &Handle) -> Result<SocketAddr> {
    let server = build_server(handle)?;
    start_server(server)
}

pub fn build_server(handle: &Handle) -> Result<Server> {
    Ok(Server::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls(SERVER_CERTS)?
        .with_event(tracing_events())?
        .with_random(Random::with_seed(123))?
        .start()?)
}

pub fn client(handle: &Handle, server_addr: SocketAddr) -> Result {
    let client = build_client(handle)?;
    start_client(client, server_addr, Data::new(10_000))
}

pub fn start_client(client: Client, server_addr: SocketAddr, data: Data) -> Result {
    primary::spawn(async move {
        let connect = Connect::new(server_addr).with_server_name("localhost");
        let mut connection = client.connect(connect).await.unwrap();

        tracing::debug!("connected with client connection: {}", connection.id());

        let stream = connection.open_bidirectional_stream().await.unwrap();
        tracing::debug!("opened client stream: {}", stream.id());

        let (mut recv, mut send) = stream.split();

        let mut send_data = data;
        let mut recv_data = data;

        primary::spawn(async move {
            while let Some(chunk) = recv.receive().await.unwrap() {
                recv_data.receive(&[chunk]);
            }
            assert!(recv_data.is_finished());
        });

        while let Some(chunk) = send_data.send_one(usize::MAX) {
            tracing::debug!("client sending {} chunk", chunk.len());
            send.send(chunk).await.unwrap();
        }
    });

    Ok(())
}

pub fn build_client(handle: &Handle) -> Result<Client> {
    Ok(Client::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls(certificates::CERT_PEM)?
        .with_event(tracing_events())?
        .with_random(Random::with_seed(123))?
        .start()?)
}

pub fn client_server(handle: &Handle) -> Result<SocketAddr> {
    let addr = server(handle)?;
    client(handle, addr)?;
    Ok(addr)
}

pub struct Random {
    inner: rand_chacha::ChaCha8Rng,
}

impl Random {
    pub fn with_seed(seed: u64) -> Self {
        use rand::SeedableRng;
        Self {
            inner: rand_chacha::ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl havoc::Random for Random {
    fn fill(&mut self, bytes: &mut [u8]) {
        self.fill_bytes(bytes);
    }

    fn gen_range(&mut self, range: std::ops::Range<u64>) -> u64 {
        self.inner.gen_range(range)
    }
}

impl RngCore for Random {
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.inner.try_fill_bytes(dest)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.inner.fill_bytes(dest)
    }

    fn next_u32(&mut self) -> u32 {
        self.inner.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }
}

impl crate::provider::random::Provider for Random {
    type Generator = Self;

    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Generator, Self::Error> {
        Ok(self)
    }
}

impl crate::provider::random::Generator for Random {
    fn public_random_fill(&mut self, dest: &mut [u8]) {
        self.fill_bytes(dest);
    }

    fn private_random_fill(&mut self, dest: &mut [u8]) {
        self.fill_bytes(dest);
    }
}

#[cfg(not(target_os = "windows"))]
mod mtls {
    use super::*;
    use crate::provider::tls;

    pub fn build_client_mtls_provider(ca_cert: &str) -> Result<tls::default::Client> {
        let tls = tls::default::Client::builder()
            .with_certificate(ca_cert)?
            .with_client_identity(
                certificates::MTLS_CLIENT_CERT,
                certificates::MTLS_CLIENT_KEY,
            )?
            .build()?;
        Ok(tls)
    }

    pub fn build_server_mtls_provider(ca_cert: &str) -> Result<tls::default::Server> {
        let tls = tls::default::Server::builder()
            .with_certificate(
                certificates::MTLS_SERVER_CERT,
                certificates::MTLS_SERVER_KEY,
            )?
            .with_client_authentication()?
            .with_trusted_certificate(ca_cert)?
            .build()?;
        Ok(tls)
    }
}

#[cfg(not(target_os = "windows"))]
pub use mtls::*;

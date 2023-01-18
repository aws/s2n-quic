// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::Connect,
    provider::{
        event,
        io::testing::{primary, spawn, Handle, Io, Result},
    },
    Client, Server,
};
use rand::{Rng, RngCore};
use s2n_quic_core::{crypto::tls::testing::certificates, havoc, stream::testing::Data, event::Subscriber};
use std::net::SocketAddr;

pub static SERVER_CERTS: (&str, &str) = (certificates::CERT_PEM, certificates::KEY_PEM);

pub fn events() -> event::tracing::Provider {
    use std::sync::Once;

    static TRACING: Once = Once::new();

    // make sure this only gets initialized once
    TRACING.call_once(|| {
        let format = tracing_subscriber::fmt::format()
            .with_level(false) // don't include levels in formatted output
            .with_timer(tracing_subscriber::fmt::time::uptime())
            .with_ansi(false)
            .compact(); // Use a less verbose output format.

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("debug"))
            .event_format(format)
            .with_test_writer()
            .init();
    });

    event::tracing::Provider::default()
}

pub fn server_with<F: FnOnce(Io) -> Result<Server>>(
    handle: &Handle,
    build: F,
) -> Result<SocketAddr> {
    let mut server = build(handle.builder().build().unwrap())?;
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            spawn(async move {
                while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                    spawn(async move {
                        while let Ok(Some(chunk)) = stream.receive().await {
                            let _ = stream.send(chunk).await;
                        }
                    });
                }
            });
        }
    });

    Ok(server_addr)
}

pub fn server(handle: &Handle) -> Result<SocketAddr> {
    server_with(handle, |io| {
        Ok(Server::builder()
            .with_io(io)?
            .with_tls(SERVER_CERTS)?
            .with_event(events())?
            .start()?)
    })
}

pub fn server_with_subscriber<S: Subscriber>(handle: &Handle, subscriber: S) -> Result<SocketAddr> {
    server_with(handle, |io| {
        Ok(Server::builder()
            .with_io(io)?
            .with_tls(SERVER_CERTS)?
            .with_event(subscriber)?
            .start()?)
    })
}

pub fn build_server(handle: &Handle) -> Result<Server> {
    Ok(Server::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls(SERVER_CERTS)?
        .with_event(events())?
        .start()?)
}

pub fn client(handle: &Handle, server_addr: SocketAddr) -> Result {
    let client = build_client(handle)?;

    primary::spawn(async move {
        let connect = Connect::new(server_addr).with_server_name("localhost");
        let mut connection = client.connect(connect).await.unwrap();

        let stream = connection.open_bidirectional_stream().await.unwrap();
        let (mut recv, mut send) = stream.split();

        let mut send_data = Data::new(10_000);

        let mut recv_data = send_data;
        primary::spawn(async move {
            while let Some(chunk) = recv.receive().await.unwrap() {
                recv_data.receive(&[chunk]);
            }
            assert!(recv_data.is_finished());
        });

        while let Some(chunk) = send_data.send_one(usize::MAX) {
            send.send(chunk).await.unwrap();
        }
    });

    Ok(())
}

pub fn build_client(handle: &Handle) -> Result<Client> {
    Ok(Client::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls(certificates::CERT_PEM)?
        .with_event(events())?
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

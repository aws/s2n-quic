// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{
    client::Connect,
    provider::{
        event,
        io::testing::{primary, spawn, Handle, Result},
    },
    stream::PeerStream,
    Client, Server,
};
use s2n_quic_core::{crypto::tls::testing::certificates, havoc, stream::testing::Data};

use rand::{Rng, RngCore};
use std::net::SocketAddr;

pub mod recorder;
#[cfg(test)]
mod tests;

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
                write!(w, "{}", s2n_quic::provider::io::testing::now())
            }
        }

        let env_filter = tracing_subscriber::EnvFilter::builder()
            .with_default_directive(tracing::Level::DEBUG.into())
            .with_env_var("S2N_LOG")
            .from_env()
            .unwrap();

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
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
        self.inner.random_range(range)
    }
}

impl RngCore for Random {
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

impl s2n_quic::provider::random::Provider for Random {
    type Generator = Self;

    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Generator, Self::Error> {
        Ok(self)
    }
}

impl s2n_quic::provider::random::Generator for Random {
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
    use s2n_quic::provider::tls;

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

mod slow_tls {
    use s2n_quic::provider::tls::Provider;
    use s2n_quic_core::crypto::tls::{slow_tls::SlowEndpoint, Endpoint};
    pub struct SlowTlsProvider<E: Endpoint> {
        pub endpoint: E,
    }

    impl<E: Endpoint> Provider for SlowTlsProvider<E> {
        type Server = SlowEndpoint<E>;
        type Client = SlowEndpoint<E>;
        type Error = String;

        fn start_server(self) -> Result<Self::Server, Self::Error> {
            Ok(SlowEndpoint::new(self.endpoint))
        }

        fn start_client(self) -> Result<Self::Client, Self::Error> {
            Ok(SlowEndpoint::new(self.endpoint))
        }
    }
}

#[cfg(unix)]
pub mod resumption {
    use super::*;
    use s2n_quic::provider::tls::{
        self,
        s2n_tls::{
            callbacks::{ConnectionFuture, SessionTicket, SessionTicketCallback},
            config::ConnectionInitializer,
            connection::Connection,
            error::Error,
            Server,
        },
    };
    use std::{
        collections::VecDeque,
        pin::Pin,
        sync::{Arc, Mutex},
    };

    pub static TICKET_KEY: [u8; 16] = [0; 16];
    #[derive(Default, Clone)]
    pub struct SessionTicketHandler {
        ticket_storage: Arc<Mutex<VecDeque<Vec<u8>>>>,
    }

    impl SessionTicketCallback for SessionTicketHandler {
        fn on_session_ticket(&self, _connection: &mut Connection, session_ticket: &SessionTicket) {
            let size = session_ticket.len().unwrap();
            let mut data = vec![0; size];
            session_ticket.data(&mut data).unwrap();
            let mut vec = (*self.ticket_storage).lock().unwrap();
            vec.push_back(data);
        }
    }

    impl ConnectionInitializer for SessionTicketHandler {
        fn initialize_connection(
            &self,
            connection: &mut Connection,
        ) -> Result<Option<Pin<Box<(dyn ConnectionFuture)>>>, Error> {
            if let Some(ticket) = (*self.ticket_storage).lock().unwrap().pop_back().as_deref() {
                connection.set_session_ticket(ticket)?;
            }
            Ok(None)
        }
    }

    pub fn build_server_resumption_provider(
        cert: &str,
        key: &str,
    ) -> Result<tls::default::Server<Server>> {
        let mut tls = Server::builder().with_certificate(cert, key)?;

        let config = tls.config_mut();
        config.enable_session_tickets(true)?;
        config.add_session_ticket_key(
            "keyname".as_bytes(),
            &TICKET_KEY,
            std::time::SystemTime::now(),
        )?;

        let tls = Server::from_loader(tls.build()?);
        Ok(tls)
    }

    pub fn build_client_resumption_provider(
        cert: &str,
        handler: &SessionTicketHandler,
    ) -> Result<tls::default::Client> {
        let mut tls = tls::s2n_tls::Client::builder().with_certificate(cert)?;
        let config = tls.config_mut();
        config
            .enable_session_tickets(true)?
            .set_session_ticket_callback(handler.clone())?
            .set_connection_initializer(handler.clone())?;
        Ok(tls.build()?)
    }
}

#[cfg(not(target_os = "windows"))]
pub use mtls::*;

pub use slow_tls::SlowTlsProvider;

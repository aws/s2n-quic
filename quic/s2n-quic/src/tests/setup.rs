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
use s2n_quic_core::{
    crypto::tls::testing::certificates, havoc, inet::ExplicitCongestionNotification::*,
    stream::testing::Data,
};
use s2n_quic_platform::io::testing::Socket;
use std::net::SocketAddr;
use zerocopy::IntoBytes;

pub static SERVER_CERTS: (&str, &str) = (certificates::CERT_PEM, certificates::KEY_PEM);

#[cfg(not(target_arch = "x86"))]
const QUICHE_MAX_DATAGRAM_SIZE: usize = 1350;
const QUICHE_STREAM_ID: u64 = 0;

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

#[cfg(not(target_arch = "x86"))]
pub fn start_quiche_client(
    mut client_conn: quiche::Connection,
    socket: Socket,
    migrated_socket: Socket,
    server_addr: SocketAddr,
) -> Result {
    let mut out = [0; QUICHE_MAX_DATAGRAM_SIZE];
    let mut buf = [0; QUICHE_MAX_DATAGRAM_SIZE];
    let application_data = "Test Migration";

    primary::spawn(async move {
        client_conn.timeout();

        // Write Initial handshake packets
        let (write, send_info) = client_conn.send(&mut out).expect("Initial send failed");
        socket
            .send_to(send_info.to, NotEct, out[..write].to_vec())
            .unwrap();

        let mut path_probed = false;
        let mut req_sent = false;
        loop {
            // We need to check if there is a timeout event at the beginning of
            // each loop to make sure that the connection will close properly when
            // the test is done.
            client_conn.on_timeout();
            // Quiche doesn't handle IO. So we need to handle events happen
            // on both the original socket and the migrated socket
            let sockets = vec![&socket, &migrated_socket];
            for active_socket in sockets {
                let local_addr = active_socket.local_addr().unwrap();
                match active_socket.try_recv_from() {
                    Ok(Some((from, _ecn, payload))) => {
                        // Quiche conn.recv requires a mutable payload array
                        let mut payload_copy = payload.clone();

                        // Feed received data from IO Socket to Quiche
                        let _read = match client_conn.recv(
                            &mut payload_copy,
                            quiche::RecvInfo {
                                from,
                                to: active_socket.local_addr().unwrap(),
                            },
                        ) {
                            Ok(v) => v,
                            Err(quiche::Error::Done) => 0,
                            Err(e) => {
                                panic!("quiche client receive error: {e:?}");
                            }
                        };
                    }
                    Ok(None) => {}
                    Err(e) => {
                        panic!("quiche client socket recv error: {e:?}");
                    }
                }

                for peer_addr in client_conn.paths_iter(local_addr) {
                    loop {
                        let (write, send_info) = match client_conn.send_on_path(
                            &mut out,
                            Some(local_addr),
                            Some(peer_addr),
                        ) {
                            Ok(v) => v,
                            Err(quiche::Error::Done) => {
                                break;
                            }
                            Err(e) => {
                                panic!("quiche client send error: {e:?}")
                            }
                        };

                        active_socket
                            .send_to(send_info.to, NotEct, out[..write].to_vec())
                            .unwrap();
                    }

                    // Send application data using the migrated address
                    // This can only be done once the connection migration is completed
                    if local_addr == migrated_socket.local_addr().unwrap()
                        && client_conn
                            .is_path_validated(local_addr, peer_addr)
                            .unwrap()
                        && !req_sent
                    {
                        client_conn
                            .stream_send(QUICHE_STREAM_ID, application_data.as_bytes(), true)
                            .unwrap();
                        req_sent = true;
                    }
                }

                for stream_id in client_conn.readable() {
                    while let Ok((read, _)) = client_conn.stream_recv(stream_id, &mut buf) {
                        let stream_buf = &buf[..read];
                        // The data that the Quiche client received should be the same that it sent
                        if stream_buf.as_bytes() == application_data.as_bytes() {
                            // The test is done once the client receives the data. Hence, close the connection
                            client_conn.close(false, 0x00, b"test finished").unwrap();
                        } else {
                            panic!("No string received!");
                        }
                    }
                }
            }

            // Exit the test once the connection is closed
            if client_conn.is_closed() {
                break;
            }

            while let Some(qe) = client_conn.path_event_next() {
                if let quiche::PathEvent::Validated(local_addr, peer_addr) = qe {
                    client_conn.migrate(local_addr, peer_addr).unwrap();
                }
            }

            // Perform connection migration after the server provides spare CIDs
            if client_conn.available_dcids() > 0 && !path_probed {
                let new_addr = migrated_socket.local_addr().unwrap();
                client_conn.probe_path(new_addr, server_addr).unwrap();
                path_probed = true;
            }

            // Sleep a bit to avoid busy-waiting
            crate::provider::io::testing::time::delay(std::time::Duration::from_millis(10)).await;
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

mod slow_tls {
    use crate::provider::tls::Provider;
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

#[cfg(feature = "s2n-quic-tls")]
mod resumption {
    use super::*;
    use crate::provider::tls::{
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
    ) -> Result<tls::default::Server<s2n_quic_tls_default::Server>> {
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

#[cfg(feature = "s2n-quic-tls")]
pub use resumption::*;

pub use slow_tls::SlowTlsProvider;

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::client::tokio::Client,
    testing::{NoopSubscriber, TestTlsProvider},
};
use s2n_quic::server::Name;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn client_config() -> s2n_tls::config::Config {
    let mut client_config = s2n_tls::config::Builder::new();
    client_config
        .with_system_certs(false)
        .unwrap()
        .trust_pem(s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes())
        .unwrap()
        .load_pem(
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes(),
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.as_bytes(),
        )
        .unwrap();
    client_config.build().unwrap()
}

fn server_config() -> s2n_tls::config::Config {
    let mut server_config = s2n_tls::config::Builder::new();
    server_config
        .with_system_certs(false)
        .unwrap()
        .set_client_auth_type(s2n_tls::enums::ClientAuthType::Required)
        .unwrap()
        .set_verify_host_callback(VerifyHostNameClientCertVerifier::new("qlaws.qlaws"))
        .unwrap()
        .load_pem(
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes(),
            s2n_quic_core::crypto::tls::testing::certificates::KEY_PEM.as_bytes(),
        )
        .unwrap()
        .trust_pem(s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.as_bytes())
        .unwrap();
    server_config.build().unwrap()
}

fn dc_client() -> Client<crate::psk::client::Provider, crate::event::tracing::Subscriber> {
    let map = crate::path::secret::Map::new(
        crate::path::secret::stateless_reset::Signer::new(b"default"),
        50,
        false,
        s2n_quic_core::time::StdClock::default(),
        crate::event::tracing::Subscriber::default(),
    );
    let hs = crate::psk::client::Provider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            map.clone(),
            TestTlsProvider {},
            NoopSubscriber {},
            Name::from_static("qlaws.qlaws"),
        )
        .unwrap();
    Client::<crate::psk::client::Provider, crate::event::tracing::Subscriber>::builder()
        .with_udp(false)
        .build(hs, crate::event::tracing::Subscriber::default())
        .unwrap()
}

async fn dc_server(
) -> crate::stream::server::tokio::Server<DummyHandshake, crate::event::tracing::Subscriber> {
    let map = crate::path::secret::Map::new(
        crate::path::secret::stateless_reset::Signer::new(b"default"),
        50,
        false,
        s2n_quic_core::time::StdClock::default(),
        crate::event::tracing::Subscriber::default(),
    );
    let hs = crate::psk::server::Provider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            TestTlsProvider {},
            NoopSubscriber {},
            map.clone(),
        )
        .await
        .unwrap();
    let handshake = DummyHandshake { hs: hs.clone() };
    crate::stream::server::tokio::Server::<DummyHandshake, crate::event::tracing::Subscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_udp(false)
        .with_tcp(true)
        .build(handshake, crate::event::tracing::Subscriber::default())
        .unwrap()
}

async fn check_client(message: &[u8]) {
    let client_config = client_config();
    let server_config = server_config();

    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let acceptor = s2n_tls_tokio::TlsAcceptor::new(server_config);
        while let Ok((conn, _)) = server.accept().await {
            conn.set_nodelay(true).unwrap();

            let mut stream = acceptor.accept(conn).await.unwrap();
            let mut buffer = vec![];
            stream.read_to_end(&mut buffer).await.unwrap();
            eprintln!("server read {} bytes", buffer.len());
            stream.write_all(&buffer).await.unwrap();
            eprintln!("server finished writing {} bytes", buffer.len());
            stream.flush().await.unwrap();
            stream.shutdown().await.unwrap();
            drop(stream);
        }
    });

    let client = dc_client();
    let stream = client
        .connect_tls(
            server_addr,
            Name::from_static("qlaws.qlaws"),
            &client_config,
        )
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();

    writer.write_all_from(&mut &message[..]).await.unwrap();
    eprintln!("finished writing");

    writer.shutdown().unwrap();

    eprintln!("writer.shutdown() done");

    let mut buffer: Vec<u8> = vec![];
    reader.read_to_end(&mut buffer).await.unwrap();
    assert_eq!(buffer, message);
}

async fn check_server(message: &[u8]) {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();
    let client_config = client_config();

    let server = dc_server().await;
    let server_hs_addr = server.handshake_addr().unwrap();
    let acceptor_addr = server.acceptor_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = server.accept().await {
            tracing::info!("server accepted stream!");
            let mut buffer = vec![];
            stream.read_to_end(&mut buffer).await.unwrap();
            eprintln!("server read {} bytes", buffer.len());
            stream.write_all(&buffer).await.unwrap();
            eprintln!("server finished writing {} bytes", buffer.len());
            stream.flush().await.unwrap();
            stream.shutdown().await.unwrap();
            drop(stream);
        }
    });

    let client = dc_client();
    let stream = client
        .connect_tls(
            acceptor_addr,
            Name::from_static("qlaws.qlaws"),
            &client_config,
        )
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();

    writer.write_all_from(&mut &message[..]).await.unwrap();
    eprintln!("finished writing");

    writer.shutdown().unwrap();

    eprintln!("writer.shutdown() done");

    let mut buffer: Vec<u8> = vec![];
    reader.read_to_end(&mut buffer).await.unwrap();
    assert_eq!(buffer, message);

    // Then confirm dcQUIC to the same endpoint also works...
    let stream = client
        .connect(
            server_hs_addr,
            acceptor_addr,
            Name::from_static("qlaws.qlaws"),
        )
        .await
        .unwrap();
    let (mut reader, mut writer) = stream.into_split();

    writer.write_all_from(&mut &message[..]).await.unwrap();
    eprintln!("finished writing");

    writer.shutdown().unwrap();

    eprintln!("writer.shutdown() done");

    let mut buffer: Vec<u8> = vec![];
    reader.read_to_end(&mut buffer).await.unwrap();
    assert_eq!(buffer, message);
}

#[tokio::test]
async fn short() {
    check_client(&b"testing"[..]).await;
}

#[tokio::test]
async fn medium() {
    let message = vec![0x3; 1024 * 1024];
    check_client(&message).await;
}

#[tokio::test]
async fn large() {
    let message = vec![0x3; 50 * 1024 * 1024];
    check_client(&message).await;
}

#[tokio::test]
async fn short_server() {
    check_server(&b"testing"[..]).await;
}

#[tokio::test]
async fn medium_server() {
    let message = vec![0x3; 1024 * 1024];
    check_server(&message).await;
}

#[tokio::test]
async fn large_server() {
    let message = vec![0x3; 50 * 1024 * 1024];
    check_server(&message).await;
}

#[tokio::test]
async fn closed_during_handshake() {
    let client_config = client_config();

    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((conn, _)) = server.accept().await {
            drop(conn);
        }
    });

    let client = dc_client();
    let err = client
        .connect_tls(
            server_addr,
            Name::from_static("qlaws.qlaws"),
            &client_config,
        )
        .await
        .expect_err("handshake failed");
    let err = format!("{:?}", err);
    assert!(err.contains("Connection reset by peer"), "{}", err);
}

#[tokio::test]
async fn incorrect_record_after_handshake() {
    let client_config = client_config();
    let server_config = server_config();

    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let acceptor = s2n_tls_tokio::TlsAcceptor::new(server_config);
        while let Ok((conn, _)) = server.accept().await {
            conn.set_nodelay(true).unwrap();

            let mut stream = acceptor.accept(conn).await.unwrap();
            let mut buffer = vec![];
            stream.read_to_end(&mut buffer).await.unwrap();

            // Bypass the s2n-tls wrapper and write raw bytes to the stream. This confirms the
            // receiver correctly handles closing the stream.
            stream.get_mut().write_all(&buffer).await.unwrap();
            stream.get_mut().flush().await.unwrap();
            stream.get_mut().shutdown().await.unwrap();

            drop(stream);
        }
    });

    let client = dc_client();
    let stream = client
        .connect_tls(
            server_addr,
            Name::from_static("qlaws.qlaws"),
            &client_config,
        )
        .await
        .unwrap();

    let message = [0x3; 1024];
    let (mut reader, mut writer) = stream.into_split();

    writer.write_all_from(&mut &message[..]).await.unwrap();
    eprintln!("finished writing");

    writer.shutdown().unwrap();

    eprintln!("writer.shutdown() done");

    let mut buffer: Vec<u8> = vec![];
    let err = reader.read_to_end(&mut buffer).await.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{:?}", err);
}

#[tokio::test]
async fn unauthenticated_closure() {
    let client_config = client_config();
    let server_config = server_config();

    let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = server.local_addr().unwrap();
    tokio::spawn(async move {
        let acceptor = s2n_tls_tokio::TlsAcceptor::new(server_config);
        while let Ok((conn, _)) = server.accept().await {
            conn.set_nodelay(true).unwrap();

            let mut stream = acceptor.accept(conn).await.unwrap();
            let mut buffer = vec![];
            stream.read_to_end(&mut buffer).await.unwrap();

            // Directly close the stream without shutting it down.
            stream.get_mut().flush().await.unwrap();
            stream.get_mut().shutdown().await.unwrap();

            // Ensure the Drop impl can't write anything either. This does leak the fd and some
            // memory but we're OK with that in test code.
            std::mem::forget(stream);
        }
    });

    let client = dc_client();
    let stream = client
        .connect_tls(
            server_addr,
            Name::from_static("qlaws.qlaws"),
            &client_config,
        )
        .await
        .unwrap();

    let message = [0x3; 1024];
    let (mut reader, mut writer) = stream.into_split();

    writer.write_all_from(&mut &message[..]).await.unwrap();
    eprintln!("finished writing");

    writer.shutdown().unwrap();

    eprintln!("writer.shutdown() done");

    let mut buffer: Vec<u8> = vec![];
    let err = reader.read_to_end(&mut buffer).await.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof, "{:?}", err);
}

#[derive(Clone)]
struct DummyHandshake {
    hs: crate::psk::server::Provider,
}

impl crate::stream::server::tokio::Handshake for DummyHandshake {
    fn local_addr(&self) -> std::net::SocketAddr {
        self.hs.local_addr()
    }

    fn map(&self) -> &crate::path::secret::Map {
        self.hs.map()
    }

    fn server_tls(&self) -> Option<crate::stream::server::tokio::tcp::tls::Builder> {
        Some(crate::stream::server::tokio::tcp::tls::Builder {
            rt: std::sync::Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap(),
            ),
            config: server_config(),
        })
    }
}

pub struct VerifyHostNameClientCertVerifier {
    host_name: String,
}

impl s2n_tls::callbacks::VerifyHostNameCallback for VerifyHostNameClientCertVerifier {
    fn verify_host_name(&self, host_name: &str) -> bool {
        self.host_name == host_name
    }
}

impl VerifyHostNameClientCertVerifier {
    pub fn new(host_name: impl ToString) -> VerifyHostNameClientCertVerifier {
        VerifyHostNameClientCertVerifier {
            host_name: host_name.to_string(),
        }
    }
}

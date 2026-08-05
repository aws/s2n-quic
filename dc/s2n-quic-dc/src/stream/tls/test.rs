// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

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
        .with_tls(crate::stream::server::tokio::tcp::tls::Builder::new(
            std::sync::Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .unwrap(),
            ),
            Arc::new(server_config()),
        ))
        .build(handshake, crate::event::tracing::Subscriber::default())
        .unwrap()
}

async fn check_client(message: &[u8]) {
    check_client_inner(message, false).await;
}

/// Same as [`check_client`], but connects using a pre-existing TCP stream via
/// [`Client::connect_tls_with`].
async fn check_client_with(message: &[u8]) {
    check_client_inner(message, true).await;
}

async fn check_client_inner(message: &[u8], use_existing_stream: bool) {
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
    let stream = if use_existing_stream {
        let conn = tokio::net::TcpStream::connect(server_addr).await.unwrap();
        client
            .connect_tls_with(conn, Name::from_static("qlaws.qlaws"), &client_config)
            .await
            .unwrap()
    } else {
        client
            .connect_tls(
                server_addr,
                Name::from_static("qlaws.qlaws"),
                &client_config,
            )
            .await
            .unwrap()
    };
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
            if let Some(chain) = stream.peer_cert_chain() {
                assert_eq!(chain.iter_der().count(), 1);
            }

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

    assert_eq!(stream.peer_cert_chain().unwrap().iter_der().count(), 1);

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

    assert!(stream.peer_cert_chain().is_none());

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
async fn short_with() {
    check_client_with(&b"testing"[..]).await;
}

#[tokio::test]
async fn medium_with() {
    let message = vec![0x3; 1024 * 1024];
    check_client_with(&message).await;
}

#[tokio::test]
async fn large_with() {
    let message = vec![0x3; 50 * 1024 * 1024];
    check_client_with(&message).await;
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

/// Appends a single `Extension { uint16 extension_type; opaque extension_data<0..2^16-1> }` to
/// `extensions`.
fn push_extension(extensions: &mut Vec<u8>, extension_type: u16, extension_data: &[u8]) {
    extensions.extend_from_slice(&extension_type.to_be_bytes());
    extensions.extend_from_slice(&(extension_data.len() as u16).to_be_bytes());
    extensions.extend_from_slice(extension_data);
}

/// Builds a single TLS 1.3 ClientHello record carrying the provided 32-byte client random.
///
/// The bytes are hand-encoded per RFC 8446 4.1.2:
///
/// ```text
/// struct {
///     ProtocolVersion legacy_version = 0x0303;    /* TLS v1.2 */
///     Random random;
///     opaque legacy_session_id<0..32>;
///     CipherSuite cipher_suites<2..2^16-2>;
///     opaque legacy_compression_methods<1..2^8-1>;
///     Extension extensions<8..2^16-1>;
/// } ClientHello;
/// ```
///
/// It is a complete enough ClientHello (advertising TLS 1.3, a mutually-supported group, and a
/// matching key share) that the server proceeds to send a ServerHello rather than a
/// HelloRetryRequest. That matters because a HelloRetryRequest causes s2n-tls to discard the
/// parsed ClientHello (and thus the client random that `is_synthetic` reads). The handshake is not
/// expected to complete -- the peer goes away -- but the client random is recorded along the way.
fn client_hello_record(random: &[u8; 32]) -> Vec<u8> {
    // The secp256r1 (NIST P-256) generator point in uncompressed form (0x04 || X || Y). It is a
    // valid point on the curve, so it serves as a well-formed key_share public key.
    const SECP256R1_GENERATOR: [u8; 65] = [
        0x04, 0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4,
        0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8,
        0x98, 0xc2, 0x96, 0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a,
        0x7c, 0x0f, 0x9e, 0x16, 0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40,
        0x68, 0x37, 0xbf, 0x51, 0xf5,
    ];
    const SECP256R1: u16 = 0x0017;

    let mut extensions = vec![];
    // supported_versions: advertise TLS 1.3 (0x0304), which is how a TLS 1.3 ClientHello is
    // identified (the legacy_version below stays at 0x0303).
    push_extension(&mut extensions, 0x002b, &[0x02, 0x03, 0x04]);
    // supported_groups: a list of one group, secp256r1.
    push_extension(&mut extensions, 0x000a, &[0x00, 0x02, 0x00, 0x17]);
    // signature_algorithms: a list of one scheme, rsa_pss_rsae_sha256 (0x0804). Not actually used
    // before the ClientHello is recorded, but a well-formed ClientHello carries one.
    push_extension(&mut extensions, 0x000d, &[0x00, 0x02, 0x08, 0x04]);
    // key_share: a single KeyShareEntry for secp256r1 carrying the generator point.
    let mut key_share = vec![];
    key_share.extend_from_slice(&SECP256R1.to_be_bytes());
    key_share.extend_from_slice(&(SECP256R1_GENERATOR.len() as u16).to_be_bytes());
    key_share.extend_from_slice(&SECP256R1_GENERATOR);
    let mut key_share_ext = (key_share.len() as u16).to_be_bytes().to_vec();
    key_share_ext.extend_from_slice(&key_share);
    push_extension(&mut extensions, 0x0033, &key_share_ext);

    // ClientHello body.
    let mut body = vec![];
    body.extend_from_slice(&[0x03, 0x03]); // legacy_version = TLS 1.2
    body.extend_from_slice(random); // Random[32]
    body.push(0x00); // legacy_session_id: empty (length 0)
    body.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]); // cipher_suites: len 2, TLS_AES_128_GCM_SHA256
    body.extend_from_slice(&[0x01, 0x00]); // legacy_compression_methods: len 1, "null"
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);

    // Handshake header: msg_type = client_hello (1), then a uint24 length.
    let mut handshake = vec![0x01];
    let len = body.len();
    handshake.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8]);
    handshake.extend_from_slice(&body);

    // Record header: content_type = handshake (22), legacy record version, then a uint16 length.
    let mut record = vec![0x16, 0x03, 0x01];
    record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
    record.extend_from_slice(&handshake);
    record
}

/// Runs the given ClientHello record through a fresh server connection at EOF and reports whether
/// it is classified as synthetic.
async fn detect_synthetic(random: &[u8; 32]) -> bool {
    // A peer that immediately closes, so the handshake reads our buffered ClientHello and then
    // hits EOF.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((conn, _)) = listener.accept().await {
            drop(conn);
        }
    });
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();

    let socket = Arc::new(crate::stream::socket::application::Single(stream));
    let conn: crate::stream::tls::Conn = Box::new(
        s2n_tls::connection::Builder::build_connection(
            &server_config(),
            s2n_tls::enums::Mode::Server,
        )
        .unwrap(),
    );
    let mut connection =
        crate::stream::tls::S2nTlsConnection::from_connection(socket, conn).unwrap();

    let buffer = crate::msg::recv::Message::new_from_packet(client_hello_record(random), addr);
    // The handshake is expected to fail (we never send a complete handshake / the peer is gone),
    // but s2n-tls parses and stores the client random first, which is what `is_synthetic` reads.
    let _ = connection.negotiate(Some(buffer)).await;

    connection.is_synthetic()
}

#[tokio::test]
async fn detects_synthetic_client_random() {
    let mut random = [0u8; 32];
    random[..b"s2n-proctor".len()].copy_from_slice(b"s2n-proctor");
    assert!(detect_synthetic(&random).await);
}

#[tokio::test]
async fn ignores_non_synthetic_client_random() {
    // A random without the synthetic marker must not be misclassified.
    let random = [0x42u8; 32];
    assert!(!detect_synthetic(&random).await);
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

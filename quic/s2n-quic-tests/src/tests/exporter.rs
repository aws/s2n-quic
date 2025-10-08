// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module shows an example of an event provider that exports a symmetric key from an s2n-quic
//! connection on both client and server.

use super::*;
use s2n_quic::provider::event::events::{
    self, CipherSuite, ConnectionInfo, ConnectionMeta, Subscriber,
};

struct Exporter;

#[derive(Default)]
struct ExporterContext {
    key: Option<[u8; 32]>,
    cipher_suite: Option<CipherSuite>,
}

impl Subscriber for Exporter {
    type ConnectionContext = ExporterContext;

    #[inline]
    fn create_connection_context(
        &mut self,
        _: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        ExporterContext::default()
    }

    fn on_tls_exporter_ready(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::TlsExporterReady,
    ) {
        let mut key = [0; 32];
        event
            .session
            .tls_exporter(b"EXPERIMENTAL EXPORTER s2n-quic", b"some context", &mut key)
            .unwrap();
        context.key = Some(key);
        context.cipher_suite = Some(event.session.cipher_suite());
    }
}

fn start_server(
    mut server: Server,
    cipher_suite: Arc<Mutex<Option<CipherSuite>>>,
) -> s2n_quic::provider::io::testing::Result<SocketAddr> {
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            let key = connection
                .query_event_context(|ctx: &ExporterContext| ctx.key.unwrap())
                .unwrap();

            let _ = cipher_suite.lock().unwrap().insert(
                connection
                    .query_event_context(|ctx: &ExporterContext| ctx.cipher_suite.unwrap())
                    .unwrap(),
            );

            tracing::debug!("accepted server connection: {}", connection.id());
            spawn(async move {
                while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                    tracing::debug!("accepted server stream: {}", stream.id());
                    spawn(async move {
                        stream.send(Bytes::from(key.to_vec())).await.unwrap();
                    });
                }
            });
        }
    });

    Ok(server_addr)
}

fn tls_test<C>(f: fn(s2n_quic::Connection, CipherSuite) -> C)
where
    C: 'static + core::future::Future<Output = ()> + Send,
{
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((Exporter, tracing_events(true, model.max_udp_payload())))?
            .start()?;
        let server_cipher_suite = Arc::new(Mutex::new(None));

        let addr = start_server(server, server_cipher_suite.clone())?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((Exporter, tracing_events(true, model.max_udp_payload())))?
            .start()?;

        // show it working for several connections
        for _ in 0..10 {
            let client = client.clone();
            let server_cipher_suite = server_cipher_suite.clone();
            primary::spawn(async move {
                let connect = Connect::new(addr).with_server_name("localhost");
                let conn = client.connect(connect).await.unwrap();
                // Wait a bit to allow the handshake to complete on the
                // server and populate the server cipher suite
                delay(Duration::from_millis(100)).await;
                let server_cipher_suite = server_cipher_suite.lock().unwrap().unwrap();
                f(conn, server_cipher_suite).await;
            });
        }

        Ok(addr)
    })
    .unwrap();
}

#[test]
fn happy_case() {
    tls_test(|mut conn, server_cipher_suite| async move {
        use s2n_quic_core::event::IntoEvent;
        let client_key = conn
            .query_event_context(|ctx: &ExporterContext| ctx.key.unwrap())
            .unwrap();

        let client_cipher_suite = conn
            .query_event_context(|ctx: &ExporterContext| ctx.cipher_suite.unwrap())
            .unwrap();

        assert_eq!(client_cipher_suite, server_cipher_suite);
        assert_ne!(
            client_cipher_suite,
            s2n_quic_core::crypto::tls::CipherSuite::Unknown.into_event()
        );
        assert_ne!(
            server_cipher_suite,
            s2n_quic_core::crypto::tls::CipherSuite::Unknown.into_event()
        );

        let mut stream = conn.open_bidirectional_stream().await.unwrap();

        let server_key = stream.receive().await.unwrap().unwrap();

        // Both the server and the client are expected to derive the same key.
        assert_eq!(client_key, &server_key[..]);
    });
}

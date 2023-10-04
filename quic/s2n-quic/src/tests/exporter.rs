// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module shows an example of an event provider that exports a symmetric key from an s2n-quic
//! connection on both client and server.

use super::*;
use crate::provider::event::events::{self, ConnectionInfo, ConnectionMeta, Subscriber};

struct Exporter;

#[derive(Default)]
struct ExporterContext {
    key: Option<[u8; 32]>,
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
    }
}

fn start_server(mut server: Server) -> crate::provider::io::testing::Result<SocketAddr> {
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            let key = connection
                .query_event_context(|ctx: &ExporterContext| ctx.key.unwrap())
                .unwrap();

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

fn tls_test<C>(f: fn(crate::Connection) -> C)
where
    C: 'static + core::future::Future<Output = ()> + Send,
{
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((Exporter, tracing_events()))?
            .start()?;

        let addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((Exporter, tracing_events()))?
            .start()?;

        // show it working for several connections
        for _ in 0..10 {
            let client = client.clone();
            primary::spawn(async move {
                let connect = Connect::new(addr).with_server_name("localhost");
                let conn = client.connect(connect).await.unwrap();
                f(conn).await;
            });
        }

        Ok(addr)
    })
    .unwrap();
}

#[test]
fn happy_case() {
    tls_test(|mut conn| async move {
        let client_key = conn
            .query_event_context(|ctx: &ExporterContext| ctx.key.unwrap())
            .unwrap();

        let mut stream = conn.open_bidirectional_stream().await.unwrap();

        let server_key = stream.receive().await.unwrap().unwrap();

        // Both the server and the client are expected to derive the same key.
        assert_eq!(client_key, &server_key[..]);
    });
}

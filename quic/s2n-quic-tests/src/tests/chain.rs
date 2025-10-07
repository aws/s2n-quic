// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module shows an example of an event provider that accesses certificate chains
//! from QUIC connections on both client and server.

use super::*;
use s2n_quic::provider::event::events::{self, ConnectionInfo, ConnectionMeta, Subscriber};

struct Chain;

#[derive(Default)]
struct ChainContext {
    chain: Option<Vec<Vec<u8>>>,
    sender: Option<tokio::sync::mpsc::Sender<Vec<Vec<u8>>>>,
}

impl Subscriber for Chain {
    type ConnectionContext = ChainContext;

    #[inline]
    fn create_connection_context(
        &mut self,
        _: &ConnectionMeta,
        _info: &ConnectionInfo,
    ) -> Self::ConnectionContext {
        ChainContext::default()
    }

    fn on_tls_exporter_ready(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::TlsExporterReady,
    ) {
        if let Some(sender) = context.sender.take() {
            sender
                .blocking_send(event.session.peer_cert_chain_der().unwrap())
                .unwrap();
        } else {
            context.chain = Some(event.session.peer_cert_chain_der().unwrap());
        }
    }
}

fn start_server(
    mut server: Server,
    server_chain: tokio::sync::mpsc::Sender<Vec<Vec<u8>>>,
) -> s2n_quic::provider::io::testing::Result<SocketAddr> {
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            let chain = connection
                .query_event_context_mut(|ctx: &mut ChainContext| {
                    if let Some(chain) = ctx.chain.take() {
                        Some(chain)
                    } else {
                        ctx.sender = Some(server_chain.clone());
                        None
                    }
                })
                .unwrap();
            if let Some(chain) = chain {
                server_chain.send(chain).await.unwrap();
            }
        }
    });

    Ok(server_addr)
}

fn tls_test<C>(f: fn(s2n_quic::Connection, Vec<Vec<u8>>) -> C)
where
    C: 'static + core::future::Future<Output = ()> + Send,
{
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(build_server_mtls_provider(certificates::MTLS_CA_CERT)?)?
            .with_event((Chain, tracing_events(true)))?
            .start()?;
        let (send, server_chain) = tokio::sync::mpsc::channel(1);
        let server_chain = Arc::new(tokio::sync::Mutex::new(server_chain));

        let addr = start_server(server, send)?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(build_client_mtls_provider(certificates::MTLS_CA_CERT)?)?
            .with_event((Chain, tracing_events(true)))?
            .start()?;

        // show it working for several connections
        for _ in 0..10 {
            let client = client.clone();
            let server_chain = server_chain.clone();
            primary::spawn(async move {
                let connect = Connect::new(addr).with_server_name("localhost");
                let conn = client.connect(connect).await.unwrap();
                delay(Duration::from_millis(100)).await;
                let server_chain = server_chain.lock().await.recv().await.unwrap();
                f(conn, server_chain).await;
            });
        }

        Ok(addr)
    })
    .unwrap();
}

#[test]
fn happy_case() {
    tls_test(|mut conn, server_chain| async move {
        let client_chain = conn
            .query_event_context_mut(|ctx: &mut ChainContext| ctx.chain.take().unwrap())
            .unwrap();
        // these are DER-encoded and we lack nice conversion functions, so just assert some simple
        // properties.
        assert!(server_chain.len() > 1);
        assert!(client_chain.len() > 1);
        assert_ne!(server_chain, client_chain);
    });
}

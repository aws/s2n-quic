// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{provider::tls::Provider, stream::PeerStream};

use crate::provider::io::testing::{spawn, Result};
use s2n_quic_core::crypto::tls::null_tls_context::{self, UserProvidedTlsContext};

pub struct NoTlsProvider {
    ctx: UserProvidedTlsContext,
}
impl NoTlsProvider {
    pub fn new(ctx: UserProvidedTlsContext) -> Self {
        Self { ctx }
    }
}
impl Provider for NoTlsProvider {
    type Server = null_tls_context::Endpoint;
    type Client = null_tls_context::Endpoint;
    type Error = String;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        Ok(null_tls_context::Endpoint(self.ctx.clone()))
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(Self::Client::default())
    }
}

#[test]
fn no_tls_test() {
    let model = Model::default();
    let ctx = UserProvidedTlsContext { conf: "foo".into() };
    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::new(ctx.clone()))?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::new(ctx))?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
}

pub fn start_server(mut server: Server) -> Result<SocketAddr> {
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            // On application layer, read data provided by TLS provider
            let ctx: Box<UserProvidedTlsContext> =
                connection.take_tls_context().unwrap().downcast().unwrap();
            // continue our logic
            assert_eq!("foo", ctx.conf);
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

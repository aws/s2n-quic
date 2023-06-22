// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::provider::tls::Provider;
use s2n_quic_core::crypto::tls::null;

#[derive(Default)]
pub struct NoTlsProvider {}

impl Provider for NoTlsProvider {
    type Server = null::Endpoint;
    type Client = null::Endpoint;
    type Error = String;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        Ok(Self::Server::default())
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        Ok(Self::Client::default())
    }
}

#[test]
fn no_tls_test() {
    let model = Model::default();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::default())?
            .with_event(events())?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(NoTlsProvider::default())?
            .with_event(events())?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
}

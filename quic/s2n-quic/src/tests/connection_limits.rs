// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic_core::connection::limits::{
    ConnectionInfo, HandshakeInfo, Limiter, Limits, UpdatableLimits,
};

#[test]
fn connection_limits() {
    struct LimitsProvider;
    impl Limiter for LimitsProvider {
        fn on_connection(&mut self, info: &ConnectionInfo) -> Limits {
            let addr: [u8; 4] = [1, 0, 0, 1];
            let port = 49153;
            assert_eq!(info.remote_address.ip(), addr);
            assert_eq!(info.remote_address.port(), port);
            Limits::default()
        }

        fn on_post_handshake(&mut self, info: &HandshakeInfo, limits: &mut UpdatableLimits) {
            let addr: [u8; 4] = [1, 0, 0, 1];
            let port = 49153;
            assert_eq!(info.remote_address.ip(), addr);
            assert_eq!(info.remote_address.port(), port);
            assert_eq!(*info.server_name.unwrap(), "localhost".into());
            assert_eq!(info.application_protocol, "h3");
            limits.stream_batch_size(100);
        }
    }

    let model = Model::default();
    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_limits(LimitsProvider)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();
}

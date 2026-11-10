// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bach::time::timeout;
use s2n_quic_core::connection::limits::{
    ConnectionInfo, HandshakeInfo, Limiter, Limits, UpdatableLimits,
};

#[test]
fn connection_limits() {
    struct LimitsProvider {
        expected_local_address: Arc<Mutex<Option<SocketAddr>>>,
    }

    impl Limiter for LimitsProvider {
        fn on_connection(&mut self, info: &ConnectionInfo) -> Limits {
            let addr: [u8; 4] = [1, 0, 0, 1];
            let port = 49153;
            assert_eq!(info.remote_address.ip(), addr);
            assert_eq!(info.remote_address.port(), port);

            Limits::default()
                .with_max_open_remote_bidirectional_streams(1)
                .unwrap()
        }

        fn on_post_handshake(&mut self, info: &HandshakeInfo, limits: &mut UpdatableLimits) {
            let addr: [u8; 4] = [1, 0, 0, 1];
            let port = 49153;
            assert_eq!(info.remote_address.ip(), addr);
            assert_eq!(info.remote_address.port(), port);
            assert_eq!(*info.server_name.unwrap(), "localhost".into());
            assert_eq!(info.application_protocol, "h3");

            let expected_local_address = self
                .expected_local_address
                .lock()
                .unwrap()
                .expect("server local address is recorded before the client connects");
            assert_eq!(
                info.local_address.to_string(),
                expected_local_address.to_string()
            );

            // Apply the limit only after verifying the bound local address.
            limits
                .with_max_open_remote_bidirectional_streams(2)
                .unwrap();
            limits.with_stream_batch_size(10);
        }
    }

    let expected_local_address = Arc::new(Mutex::new(None));
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events(true, model.clone()))?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events(true, model.clone()))?
            .with_limits(LimitsProvider {
                expected_local_address: expected_local_address.clone(),
            })?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_event(tracing_events(true, model.clone()))?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;
        let server_addr = start_server(server)?;
        *expected_local_address.lock().unwrap() = Some(server_addr);

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");
            let mut connection = client.connect(connect).await.unwrap();

            let first = connection.open_bidirectional_stream().await.unwrap();
            let second = timeout(
                Duration::from_secs(5),
                connection.open_bidirectional_stream(),
            )
            .await
            .expect("local-address-selected stream limit should permit a second stream")
            .unwrap();
            drop((first, second));
        });

        Ok(server_addr)
    })
    .unwrap();
}

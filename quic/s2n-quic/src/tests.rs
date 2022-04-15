// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::Connect,
    provider::io::testing::{spawn, spawn_primary, time::delay, Executor, Handle, Model},
    Client, Server,
};
use s2n_quic_core::{crypto::tls::testing::certificates, stream::testing::Data};
use std::{net::SocketAddr, sync::Once, time::Duration};

type Error = Box<dyn 'static + std::error::Error>;
type Result<T = (), E = Error> = core::result::Result<T, E>;

fn setup<F: FnOnce(&Handle) -> Result<O>, O>(network: Model, f: F) {
    setup_seed(network, 123456789, f)
}

fn setup_seed<F: FnOnce(&Handle) -> Result<O>, O>(network: Model, seed: u64, f: F) {
    static TRACING: Once = Once::new();

    // make sure this only gets initialized once
    TRACING.call_once(|| {
        let format = tracing_subscriber::fmt::format()
            .with_level(false) // don't include levels in formatted output
            .with_timer(tracing_subscriber::fmt::time::uptime())
            .with_ansi(false)
            .compact(); // Use a less verbose output format.

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("debug"))
            .event_format(format)
            .with_test_writer()
            .init();
    });

    let mut executor = Executor::new(network, seed);
    let handle = executor.handle().clone();

    let result = executor.enter(|| f(&handle));

    if let Err(err) = result {
        panic!("{:?}", err);
    }

    executor.run();
}

fn server(handle: &Handle) -> Result<SocketAddr> {
    let mut server = Server::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls((certificates::CERT_PEM, certificates::KEY_PEM))?
        .with_event(crate::provider::event::tracing::Provider::default())?
        .start()?;
    let server_addr = server.local_addr()?;

    // accept connections and echo back
    spawn(async move {
        while let Some(mut connection) = server.accept().await {
            spawn(async move {
                while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                    spawn(async move {
                        while let Ok(Some(chunk)) = stream.receive().await {
                            let _ = stream.send(chunk).await;
                        }
                    });
                }
            });
        }
    });

    Ok(server_addr)
}

fn client(handle: &Handle, server_addr: SocketAddr) -> Result {
    let client = Client::builder()
        .with_io(handle.builder().build().unwrap())?
        .with_tls(certificates::CERT_PEM)?
        .with_event(crate::provider::event::tracing::Provider::default())?
        .start()?;

    spawn_primary(async move {
        let connect = Connect::new(server_addr).with_server_name("localhost");
        let mut connection = client.connect(connect).await.unwrap();

        let stream = connection.open_bidirectional_stream().await.unwrap();
        let (mut recv, mut send) = stream.split();

        let mut send_data = Data::new(10_000);

        let mut recv_data = send_data;
        spawn_primary(async move {
            while let Some(chunk) = recv.receive().await.unwrap() {
                recv_data.receive(&[chunk]);
            }
            assert!(recv_data.is_finished());
        });

        while let Some(chunk) = send_data.send_one(usize::MAX) {
            send.send(chunk).await.unwrap();
        }
    });

    Ok(())
}

fn client_server(handle: &Handle) -> Result<SocketAddr> {
    let addr = server(handle)?;
    client(handle, addr)?;
    Ok(addr)
}

#[test]
fn client_server_test() {
    setup(Model::default(), client_server);
}

fn blackhole(model: Model, blackhole_duration: Duration) {
    setup(model.clone(), |handle| {
        spawn(async move {
            // switch back and forth between blackhole and not
            loop {
                delay(blackhole_duration).await;
                // drop all packets
                model.set_drop_rate(1.0);

                delay(blackhole_duration).await;
                model.set_drop_rate(0.0);
            }
        });
        client_server(handle)
    })
}

#[test]
fn blackhole_success_test() {
    let model = Model::default();

    let network_delay = Duration::from_millis(1000);
    model.set_delay(network_delay);

    // setting the blackhole time to `network_delay / 2` causes the connection to
    // succeed
    let blackhole_duration = network_delay / 2;
    blackhole(model, blackhole_duration);
}

#[test]
#[should_panic]
fn blackhole_failure_test() {
    let model = Model::default();

    let network_delay = Duration::from_millis(1000);
    model.set_delay(network_delay);

    // setting the blackhole time to `network_delay / 2 + 1` causes the connection to fail
    let blackhole_duration = network_delay / 2 + Duration::from_millis(1);
    blackhole(model, blackhole_duration);
}

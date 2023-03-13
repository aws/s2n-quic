// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

use s2n_quic::{
    client::Connect,
    provider::{event, io},
    Client, Server,
};
use std::net::SocketAddr;
use turmoil::{lookup, Builder, Result};

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
/// NOTE: this certificate is to be used for demonstration purposes only!
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

#[test]
fn lossy_handshake() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(20))
        .build();

    sim.host("server", || async move {
        let io = io::turmoil::Builder::default()
            .with_address(bind_to(443))?
            .build()?;

        let mut server = Server::builder()
            .with_io(io)?
            .with_tls((CERT_PEM, KEY_PEM))?
            .with_event(events())?
            .start()?;

        while let Some(mut connection) = server.accept().await {
            tokio::spawn(async move {
                eprintln!("Connection accepted from {:?}", connection.remote_addr());

                while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                    tokio::spawn(async move {
                        eprintln!("Stream opened from {:?}", stream.connection().remote_addr());

                        // echo any data back to the stream
                        while let Ok(Some(data)) = stream.receive().await {
                            stream.send(data).await.expect("stream should be open");
                        }
                    });
                }
            });
        }

        Ok(())
    });

    sim.client("client", async move {
        let io = io::turmoil::Builder::default()
            .with_address(bind_to(1234))?
            .build()?;

        let client = Client::builder()
            .with_io(io)?
            .with_tls(CERT_PEM)?
            .with_event(events())?
            .start()?;

        // drop packets for 1 second
        drop_for(1);

        // even though we're dropping packets, the connection still goes through
        let server_addr: SocketAddr = (lookup("server"), 443).into();
        let mut connection = client
            .connect(Connect::new(server_addr).with_server_name("localhost"))
            .await?;

        // drop packets for 5 seconds
        drop_for(5);

        // even though we're dropping packets, the stream should still complete
        let mut stream = connection.open_bidirectional_stream().await?;
        stream.send(vec![1, 2, 3].into()).await?;
        stream.finish()?;

        let response = stream.receive().await?.unwrap();
        assert_eq!(&response[..], &[1, 2, 3]);

        Ok(())
    });

    sim.run()?;

    Ok(())
}

pub fn events() -> event::tracing::Provider {
    use std::sync::Once;

    static TRACING: Once = Once::new();

    // make sure this only gets initialized once
    TRACING.call_once(|| {
        use tokio::time::Instant;

        struct TokioUptime {
            epoch: Instant,
        }

        impl Default for TokioUptime {
            fn default() -> Self {
                Self {
                    epoch: Instant::now(),
                }
            }
        }

        impl tracing_subscriber::fmt::time::FormatTime for TokioUptime {
            fn format_time(
                &self,
                w: &mut tracing_subscriber::fmt::format::Writer,
            ) -> std::fmt::Result {
                write!(w, "{:?}", self.epoch.elapsed())
            }
        }

        let format = tracing_subscriber::fmt::format()
            .with_level(false) // don't include levels in formatted output
            .with_timer(TokioUptime::default())
            .with_ansi(false)
            .compact(); // Use a less verbose output format.

        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("trace"))
            .event_format(format)
            .with_test_writer()
            .init();
    });

    event::tracing::Provider::default()
}

fn bind_to(port: u16) -> SocketAddr {
    (std::net::Ipv4Addr::UNSPECIFIED, port).into()
}

fn drop_for(secs: u64) {
    turmoil::partition("client", "server");
    tokio::spawn(async move {
        sleep_ms(secs * 1000).await;
        turmoil::repair("client", "server");
    });
}

async fn sleep_ms(millis: u64) {
    tokio::time::sleep(core::time::Duration::from_millis(millis)).await
}

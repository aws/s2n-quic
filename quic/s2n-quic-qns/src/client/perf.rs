// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{tls, tls::TlsProviders, Result};
use futures::future::try_join_all;
use s2n_quic::{
    client,
    provider::{event, io},
    Client, Connection,
};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Perf {
    #[structopt(short, long, default_value = "127.0.0.1")]
    ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(short, long)]
    server_name: Option<String>,

    #[structopt(long)]
    ca: Option<PathBuf>,

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    application_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(short, long, default_value = "::")]
    local_ip: std::net::IpAddr,

    #[structopt(long, default_value)]
    tls: TlsProviders,
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        let mut client = self.client()?;

        let mut requests = vec![];

        // TODO support a richer connection strategy
        for _ in 0..self.connections.unwrap_or(1) {
            let mut connect = client::Connect::new((self.ip, self.port));
            if let Some(server_name) = self.server_name.as_deref() {
                connect = connect.with_server_name(server_name);
            } else {
                // TODO allow skipping setting the server_name
                connect = connect.with_server_name("localhost");
            }
            let connection = client.connect(connect).await?;

            requests.push(handle_connection(connection));
        }

        try_join_all(requests).await?;
        client.wait_idle().await?;

        return Ok(());

        async fn handle_connection(connection: Connection) -> Result<()> {
            let (_handle, acceptor) = connection.split();
            let (bidi, uni) = acceptor.split();

            let bidi = tokio::spawn(async move {
                let _ = bidi;
                // TODO implement requests
                <Result<()>>::Ok(())
            });

            let uni = tokio::spawn(async move {
                let _ = uni;
                // TODO implement requests
                <Result<()>>::Ok(())
            });

            let (bidi, uni) = futures::try_join!(bidi, uni)?;
            bidi?;
            uni?;

            Ok(())
        }
    }

    fn client(&self) -> Result<Client> {
        // TODO support specifying a local addr
        let mut io_builder =
            io::Default::builder().with_receive_address((self.local_ip, 0u16).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let client = Client::builder()
            .with_io(io)?
            .with_event(event::disabled::Provider)?;
        let client = match self.tls {
            #[cfg(unix)]
            TlsProviders::S2N => {
                let tls = s2n_quic::provider::tls::s2n_tls::Client::builder()
                    .with_certificate(tls::s2n::ca(self.ca.as_ref())?)?
                    .with_application_protocols(
                        self.application_protocols.iter().map(String::as_bytes),
                    )?
                    .with_key_logging()?
                    .build()?;
                client.with_tls(tls)?.start().unwrap()
            }
            TlsProviders::Rustls => {
                let tls = s2n_quic::provider::tls::rustls::Client::builder()
                    .with_certificate(tls::rustls::ca(self.ca.as_ref())?)?
                    .with_application_protocols(
                        self.application_protocols.iter().map(String::as_bytes),
                    )?
                    .with_key_logging()?
                    .build()?;
                client.with_tls(tls)?.start().unwrap()
            }
        };

        Ok(client)
    }
}

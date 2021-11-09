// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use futures::future::try_join_all;
use s2n_quic::{
    client,
    provider::{
        event, io,
        tls::default::certificate::{Certificate, IntoCertificate},
    },
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
    hostname: Option<String>,

    #[structopt(long)]
    ca: Option<PathBuf>,

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00.txt#2.1
    //# The ALPN used by the QUIC performance protocol is "perf".
    #[structopt(long, default_value = "perf")]
    alpn_protocols: Vec<String>,

    #[structopt(long)]
    connections: Option<usize>,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(short, long, default_value = "::")]
    local_ip: std::net::IpAddr,
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        let client = self.client()?;

        let mut requests = vec![];

        // TODO support a richer connection strategy
        for _ in 0..self.connections.unwrap_or(1) {
            let mut connect = client::Connect::new((self.ip, self.port));
            if let Some(hostname) = self.hostname.as_deref() {
                connect = connect.with_hostname(hostname);
            } else {
                // TODO allow skipping setting the hostname
                connect = connect.with_hostname("localhost");
            }
            let connection = client.connect(connect).await?;

            requests.push(handle_connection(connection));
        }

        try_join_all(requests).await?;

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
        let ca = self.ca()?;

        let tls = s2n_quic::provider::tls::default::Client::builder()
            .with_certificate(ca)?
            .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
            .with_key_logging()?
            .build()?;

        // TODO support specifying a local addr
        let mut io_builder =
            io::Default::builder().with_receive_address((self.local_ip, 0u16).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let client = Client::builder()
            .with_io(io)?
            .with_tls(tls)?
            .with_event(event::disabled::Provider)?
            .start()
            .unwrap();

        Ok(client)
    }

    fn ca(&self) -> Result<Certificate> {
        Ok(if let Some(pathbuf) = self.ca.as_ref() {
            pathbuf.into_certificate()?
        } else {
            s2n_quic_core::crypto::tls::testing::certificates::CERT_PEM.into_certificate()?
        })
    }
}

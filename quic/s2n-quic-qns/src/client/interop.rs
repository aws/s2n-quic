// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{interop, Result};
use futures::future::try_join_all;
use s2n_quic::{
    client::Connect,
    connection::Handle,
    provider::{
        event, io,
        tls::default::certificate::{Certificate, IntoCertificate},
    },
    Client,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use structopt::StructOpt;
use tokio::{fs::File, spawn};

#[derive(Debug, StructOpt)]
pub struct Interop {
    #[structopt(short, long, default_value = "127.0.0.1")]
    ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(short, long)]
    hostname: Option<String>,

    #[structopt(long)]
    ca: Option<PathBuf>,

    #[structopt(long, default_value = "hq-interop")]
    alpn_protocols: Vec<String>,

    #[structopt(long)]
    download_dir: Option<PathBuf>,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(short, long, default_value = "0.0.0.0")]
    local_ip: std::net::IpAddr,

    #[structopt(min_values = 1, required = true)]
    requests: Vec<String>,
}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        self.check_testcase();

        let client = self.client()?;

        let mut connect = Connect::new((self.ip, self.port));
        if let Some(hostname) = self.hostname.as_deref() {
            connect = connect.with_hostname(hostname);
        } else {
            // TODO make this optional?
            connect = connect.with_hostname("localhost");
        }

        let download_dir = Arc::new(self.download_dir.clone());

        if self.requests.len() > 1 && download_dir.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "`--download-dir` must be specified if there is more than one request",
            )
            .into());
        }

        // https://github.com/marten-seemann/quic-interop-runner#test-cases
        // Handshake Loss (multiconnect): Tests resilience of the handshake to high loss.
        // The client is expected to establish multiple connections, sequential or in parallel,
        // and use each connection to download a single file.
        if let Some("multiconnect") = std::env::var("TESTCASE").ok().as_deref() {
            for request in &self.requests {
                create_connection(
                    client.clone(),
                    connect.clone(),
                    core::iter::once(request.clone()),
                    download_dir.clone(),
                )
                .await?;
            }
        } else {
            create_connection(
                client.clone(),
                connect.clone(),
                self.requests.clone(),
                download_dir,
            )
            .await?;
        }

        return Ok(());

        async fn create_connection<R: IntoIterator<Item = String>>(
            client: Client,
            connect: Connect,
            requests: R,
            download_dir: Arc<Option<PathBuf>>,
        ) -> Result<()> {
            eprintln!("connecting {:?}", connect);
            let connection = client.connect(connect).await?;

            let mut streams = vec![];
            for request in requests {
                streams.push(spawn(create_stream(
                    connection.handle(),
                    request,
                    download_dir.clone(),
                )));
            }

            try_join_all(streams).await?;

            Ok(())
        }

        async fn create_stream(
            mut connection: Handle,
            request: String,
            download_dir: Arc<Option<PathBuf>>,
        ) -> Result<()> {
            eprintln!("GET {:?}", request);
            let stream = connection.open_bidirectional_stream().await?;
            let (mut rx_stream, tx_stream) = stream.split();

            interop::write_request(tx_stream, &request).await?;

            if let Some(download_dir) = download_dir.as_ref() {
                let mut abs_path = download_dir.to_path_buf();
                abs_path.push(Path::new(&request));
                let mut file = File::open(&abs_path).await?;
                tokio::io::copy(&mut rx_stream, &mut file).await?;
            } else {
                let mut stdout = tokio::io::stdout();
                tokio::io::copy(&mut rx_stream, &mut stdout).await?;
            };

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

    fn check_testcase(&self) {
        let is_supported = match std::env::var("TESTCASE").ok().as_deref() {
            Some("versionnegotiation") => false,
            Some("handshake") => true,
            Some("transfer") => true,
            // TODO enable _only_ chacha20
            Some("chacha20") => false,
            // TODO figure out how to trigger a key update
            Some("keyupdate") => false,
            // TODO support retry packets on the client
            Some("retry") => false,
            Some("resumption") => false,
            Some("zerortt") => false,
            Some("http3") => false,
            Some("multiconnect") => true,
            Some("handshakecorruption") => true,
            Some("transfercorruption") => true,
            Some("ecn") => true,
            Some("crosstraffic") => true,
            Some("rebind-addr") => true,
            Some("rebind-port") => true,
            // TODO support active connection migration
            Some("connectionmigration") => false,
            None => true,
            _ => false,
        };

        if !is_supported {
            eprintln!("unsupported");
            std::process::exit(127);
        }
    }
}

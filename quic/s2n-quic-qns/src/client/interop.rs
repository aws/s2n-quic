// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    interop::{self, Testcase},
    tls,
    tls::TlsProviders,
    Result,
};
use futures::future::try_join_all;
use s2n_quic::{
    client::Connect,
    connection::Handle,
    provider::{event, io},
    Client,
};
use std::{
    collections::{hash_map::Entry, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};
use structopt::StructOpt;
use tokio::{fs::File, io::AsyncWriteExt, net::lookup_host, spawn};
use url::{Host, Url};

#[derive(Debug, StructOpt)]
pub struct Interop {
    #[structopt(short, long)]
    ip: Option<std::net::IpAddr>,

    #[structopt(short, long, default_value = "443")]
    port: u16,

    #[structopt(long)]
    ca: Option<PathBuf>,

    #[structopt(long, default_value = "hq-interop")]
    alpn_protocols: Vec<String>,

    #[structopt(long)]
    download_dir: Option<PathBuf>,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(short, long, default_value = "::")]
    local_ip: std::net::IpAddr,

    #[structopt(long, env = "TESTCASE", possible_values = &Testcase::supported(is_supported_testcase))]
    testcase: Option<Testcase>,

    #[structopt(min_values = 1, required = true)]
    requests: Vec<Url>,

    #[structopt(long, default_value = "s2n-tls")]
    tls: TlsProviders,
}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        let mut client = self.client()?;

        let download_dir = Arc::new(self.download_dir.clone());
        if self.requests.len() > 1 && download_dir.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "`--download-dir` must be specified if there is more than one request",
            )
            .into());
        }

        let endpoints = self.endpoints().await?;

        // https://github.com/marten-seemann/quic-interop-runner#test-cases
        // Handshake Loss (multiconnect): Tests resilience of the handshake to high loss.
        // The client is expected to establish multiple connections, sequential or in parallel,
        // and use each connection to download a single file.
        if let Some(Testcase::Multiconnect) = self.testcase {
            for request in &self.requests {
                let connect = endpoints.get(&request.host().unwrap()).unwrap().clone();
                let requests = core::iter::once(request.path().to_string());
                create_connection(client.clone(), connect, requests, download_dir.clone()).await?;
            }
        } else {
            // establish a connection per endpoint rather than per request
            for (host, connect) in endpoints.iter() {
                let host = host.clone();
                let connect = connect.clone();
                let requests = self
                    .requests
                    .iter()
                    .filter_map(|req| {
                        if req.host().as_ref() == Some(&host) {
                            Some(req.path().to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                create_connection(client.clone(), connect, requests, download_dir.clone()).await?;
            }
        }

        client.wait_idle().await?;

        return Ok(());

        async fn create_connection<R: IntoIterator<Item = String>>(
            client: Client,
            connect: Connect,
            requests: R,
            download_dir: Arc<Option<PathBuf>>,
        ) -> Result<()> {
            eprintln!("connecting to {:#}", connect);
            let connection = client.connect(connect).await?;

            let mut streams = vec![];
            for request in requests {
                streams.push(spawn(create_stream(
                    connection.handle(),
                    request,
                    download_dir.clone(),
                )));
            }

            for result in try_join_all(streams).await? {
                // `try_join_all` should be returning an Err if any stream fails, but it
                // seems to just include the Err in the Vec of results. This will force
                // any Error to bubble up so it can be printed in the output.
                result?;
            }

            Ok(())
        }

        async fn create_stream(
            connection: Handle,
            request: String,
            download_dir: Arc<Option<PathBuf>>,
        ) -> Result<()> {
            eprintln!("GET {}", request);

            match create_stream_inner(connection, &request, download_dir).await {
                Ok(()) => {
                    eprintln!("Request {} completed successfully", request);
                    Ok(())
                }
                Err(error) => {
                    eprintln!("Request {} failed: {:?}", request, error);
                    Err(error)
                }
            }
        }

        async fn create_stream_inner(
            mut connection: Handle,
            request: &str,
            download_dir: Arc<Option<PathBuf>>,
        ) -> Result<()> {
            let stream = connection.open_bidirectional_stream().await?;
            let (mut rx_stream, tx_stream) = stream.split();

            interop::write_request(tx_stream, request).await?;

            if let Some(download_dir) = download_dir.as_ref() {
                if download_dir == Path::new("/dev/null") {
                    crate::perf::handle_receive_stream(rx_stream).await?;
                } else {
                    let mut abs_path = download_dir.to_path_buf();
                    abs_path.push(Path::new(request.trim_start_matches('/')));
                    let mut file = File::create(&abs_path).await?;
                    tokio::io::copy(&mut rx_stream, &mut file).await?;
                    file.flush().await?;
                }
            } else {
                let mut stdout = tokio::io::stdout();
                tokio::io::copy(&mut rx_stream, &mut stdout).await?;
                stdout.flush().await?;
            };

            Ok(())
        }
    }

    fn client(&self) -> Result<Client> {
        let mut io_builder =
            io::Default::builder().with_receive_address((self.local_ip, 0u16).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let client = Client::builder()
            .with_io(io)?
            .with_event(event::tracing::Provider::default())?;
        let client = match self.tls {
            #[cfg(unix)]
            TlsProviders::S2N => {
                let tls = s2n_quic::provider::tls::s2n_tls::Client::builder()
                    .with_certificate(tls::s2n::ca(self.ca.as_ref())?)?
                    // the "amplificationlimit" tests generates a very large chain so bump the limit
                    .with_max_cert_chain_depth(10)?
                    .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
                    .with_key_logging()?
                    .build()?;
                client.with_tls(tls)?.start().unwrap()
            }
            TlsProviders::Rustls => {
                let tls = s2n_quic::provider::tls::rustls::Client::builder()
                    .with_certificate(tls::rustls::ca(self.ca.as_ref())?)?
                    // the "amplificationlimit" tests generates a very large chain so bump the limit
                    .with_max_cert_chain_depth(10)?
                    .with_alpn_protocols(self.alpn_protocols.iter().map(String::as_bytes))?
                    .with_key_logging()?
                    .build()?;
                client.with_tls(tls)?.start().unwrap()
            }
        };

        Ok(client)
    }

    async fn endpoints(&self) -> Result<HashMap<Host<&str>, Connect>> {
        let mut endpoints = HashMap::new();

        for req in &self.requests {
            if let Some(host) = req.host() {
                if let Entry::Vacant(entry) = endpoints.entry(host.clone()) {
                    let (ip, hostname) = match host {
                        Host::Domain(domain) => {
                            let ip = if let Some(ip) = self.ip {
                                ip
                            } else {
                                lookup_host((domain, self.port))
                                    .await?
                                    .next()
                                    .unwrap_or_else(|| {
                                        panic!("host {:?} did not resolve to any addresses", domain)
                                    })
                                    .ip()
                            };

                            (ip, Some(domain))
                        }
                        Host::Ipv4(ip) => (ip.into(), None),
                        Host::Ipv6(ip) => (ip.into(), None),
                    };

                    let port = req.port().unwrap_or(self.port);

                    let connect = Connect::new((ip, port));

                    let connect = if let Some(hostname) = hostname {
                        connect.with_hostname(hostname)
                    } else {
                        // TODO make it optional
                        connect.with_hostname("localhost")
                    };

                    entry.insert(connect);
                }
            }
        }

        Ok(endpoints)
    }
}

fn is_supported_testcase(testcase: Testcase) -> bool {
    use Testcase::*;
    match testcase {
        // TODO add the ability to override the QUIC version
        VersionNegotiation => false,
        Handshake => true,
        Transfer => true,
        // TODO enable _only_ chacha20 on supported ciphersuites
        ChaCha20 => false,
        // TODO add the ability to trigger a key update from the application
        KeyUpdate => false,
        Retry => true,
        // TODO support storing tickets
        Resumption => false,
        // TODO implement 0rtt
        ZeroRtt => false,
        // TODO integrate a H3 implementation
        Http3 => false,
        Multiconnect => true,
        Ecn => true,
        // TODO support the ability to actively migrate on the client
        ConnectionMigration => false,
    }
}

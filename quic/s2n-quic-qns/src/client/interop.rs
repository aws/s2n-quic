// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    client::{h09, h3},
    interop::Testcase,
    tls,
    tls::TlsProviders,
    Result,
};
use core::time::Duration;
use s2n_quic::{
    client::Connect,
    provider::{event, io},
    Client,
};
use std::{
    collections::{hash_map::Entry, HashMap},
    path::PathBuf,
    sync::Arc,
};
use structopt::StructOpt;
use tokio::net::lookup_host;
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
    application_protocols: Vec<String>,

    #[structopt(long)]
    download_dir: Option<PathBuf>,

    #[structopt(long)]
    disable_gso: bool,

    #[structopt(long, parse(try_from_str = parse_duration))]
    keep_alive: Option<Duration>,

    #[structopt(short, long, default_value = "::")]
    local_ip: std::net::IpAddr,

    #[structopt(long, env = "TESTCASE", possible_values = &Testcase::supported(is_supported_testcase))]
    testcase: Option<Testcase>,

    #[structopt(min_values = 1, required = true)]
    requests: Vec<Url>,

    #[structopt(long, default_value)]
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
                let requests = core::iter::once(request);
                h09::create_connection(
                    client.clone(),
                    connect,
                    requests,
                    download_dir.clone(),
                    self.keep_alive,
                )
                .await?;
            }
        } else {
            // establish a connection per endpoint rather than per request
            for (host, connect) in endpoints.iter() {
                let host = host.clone();
                let connect = connect.clone();
                let requests = self
                    .requests
                    .iter()
                    .filter(|req| req.host().as_ref() == Some(&host))
                    .collect::<Vec<_>>();

                if let Some(Testcase::Http3) = self.testcase {
                    h3::create_connection(
                        client.clone(),
                        connect,
                        requests,
                        download_dir.clone(),
                        self.keep_alive,
                    )
                    .await?;
                } else {
                    h09::create_connection(
                        client.clone(),
                        connect,
                        requests,
                        download_dir.clone(),
                        self.keep_alive,
                    )
                    .await?;
                }
            }
        }

        client.wait_idle().await?;

        Ok(())
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
            .with_event(event::tracing::Subscriber::default())?;
        let client = match self.tls {
            #[cfg(unix)]
            TlsProviders::S2N => {
                let tls = s2n_quic::provider::tls::s2n_tls::Client::builder()
                    .with_certificate(tls::s2n::ca(self.ca.as_ref())?)?
                    // the "amplificationlimit" tests generates a very large chain so bump the limit
                    .with_max_cert_chain_depth(10)?
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
                    // the "amplificationlimit" tests generates a very large chain so bump the limit
                    .with_max_cert_chain_depth(10)?
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

    async fn endpoints(&self) -> Result<HashMap<Host<&str>, Connect>> {
        let mut endpoints = HashMap::new();

        for req in &self.requests {
            if let Some(host) = req.host() {
                if let Entry::Vacant(entry) = endpoints.entry(host.clone()) {
                    let (ip, server_name) = match host {
                        Host::Domain(domain) => {
                            let ip = if let Some(ip) = self.ip {
                                ip
                            } else {
                                lookup_host((domain, self.port))
                                    .await?
                                    .next()
                                    .unwrap_or_else(|| {
                                        panic!("host {domain:?} did not resolve to any addresses")
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

                    let connect = if let Some(server_name) = server_name {
                        connect.with_server_name(server_name)
                    } else {
                        // TODO make it optional
                        connect.with_server_name("localhost")
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
        Http3 => true,
        Multiconnect => true,
        Ecn => true,
        // TODO support the ability to actively migrate on the client
        ConnectionMigration => false,
    }
}

fn parse_duration(duration: &str) -> Result<Duration> {
    let seconds = duration.parse()?;
    Ok(Duration::from_secs(seconds))
}

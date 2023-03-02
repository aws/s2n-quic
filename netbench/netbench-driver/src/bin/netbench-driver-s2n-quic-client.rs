// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::Result;
use netbench_driver::Allocator;
use s2n_quic::provider::io;
use std::collections::HashSet;
use structopt::StructOpt;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    Client::from_args().run().await.map_err(|e| e.to_string())
}

#[derive(Debug, StructOpt)]
pub struct Client {
    #[structopt(flatten)]
    opts: netbench_driver::Client,

    #[structopt(long, default_value = "9001", env = "MAX_MTU")]
    max_mtu: u16,

    #[structopt(long, env = "DISABLE_GSO")]
    disable_gso: bool,
}

impl Client {
    pub async fn run(&self) -> Result<()> {
        let addresses = self.opts.address_map().await?;
        let scenario = self.opts.scenario();

        let client = self.client()?;
        let client = netbench::Client::new(client, &scenario, &addresses);
        let mut trace = self.opts.trace();
        let mut checkpoints = HashSet::new();
        let mut timer = netbench::timer::Tokio::default();
        let mut client = client.run(&mut trace, &mut checkpoints, &mut timer).await?;

        client.wait_idle().await?;

        Ok(())
    }

    fn client(&self) -> Result<s2n_quic::Client> {
        let mut tls = s2n_quic::provider::tls::default::Client::builder()
            // handle larger cert chains
            .with_max_cert_chain_depth(10)?
            .with_application_protocols(
                self.opts.application_protocols.iter().map(String::as_bytes),
            )?
            .with_key_logging()?;

        for ca in self.opts.certificate_authorities() {
            tls = tls.with_certificate(ca.pem.as_str())?;
        }

        let tls = tls.build()?;

        let mut io_builder =
            io::Default::builder().with_max_mtu(self.max_mtu)?.with_receive_address((self.opts.local_ip, 0u16).into())?;

        if self.disable_gso {
            io_builder = io_builder.with_gso_disabled()?;
        }

        let io = io_builder.build()?;

        let client = s2n_quic::Client::builder()
            .with_io(io)?
            .with_tls(tls)?
            .start()
            .unwrap();

        Ok(client)
    }
}

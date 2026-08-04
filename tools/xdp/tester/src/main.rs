// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use aya::{
    programs::{Xdp, XdpMode},
    Ebpf,
};
use aya_log::EbpfLogger;
use clap::Parser;
use log::{info, warn};
use tokio::signal;

#[derive(Debug, Parser)]
struct Opt {
    /// The interface to run the program on
    #[clap(short, long, default_value = "lo")]
    iface: String,

    /// Traces BPF events
    #[clap(long)]
    trace: bool,

    /// Exits after attaching the BPF program
    ///
    /// This can be used to validate the correctness of the BPF program.
    #[clap(long)]
    exit_on_load: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    env_logger::init();

    let bpf = if opt.trace {
        s2n_quic_xdp::bpf::DEFAULT_PROGRAM_TRACE
    } else {
        s2n_quic_xdp::bpf::DEFAULT_PROGRAM
    };

    let mut bpf = Ebpf::load(bpf)?;

    let logger = if opt.trace {
        match EbpfLogger::init(&mut bpf) {
            Ok(logger) => Some(logger),
            Err(e) => {
                warn!("failed to initialize eBPF logger: {e}");
                None
            }
        }
    } else {
        None
    };

    let program: &mut Xdp = bpf
        .program_mut(s2n_quic_xdp::bpf::PROGRAM_NAME)
        .unwrap()
        .try_into()?;
    program.load()?;

    if opt.exit_on_load {
        return Ok(());
    }

    program.attach(&opt.iface, XdpMode::default())
        .context("failed to attach the XDP program with default mode - try changing XdpMode::default() to XdpMode::Skb")?;

    // Continuously drain the eBPF trace logs to the configured logger.
    if let Some(logger) = logger {
        let mut logger =
            tokio::io::unix::AsyncFd::with_interest(logger, tokio::io::Interest::READABLE)?;
        tokio::spawn(async move {
            loop {
                let mut guard = logger
                    .readable_mut()
                    .await
                    .expect("failed to poll eBPF logger");
                guard.get_inner_mut().flush();
                guard.clear_ready();
            }
        });
    }

    info!("Waiting for Ctrl-C...");
    signal::ctrl_c().await?;
    info!("Exiting...");

    Ok(())
}

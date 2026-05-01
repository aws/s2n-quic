// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod busy_poll;
mod client;
mod config;
mod psk;
mod server;
mod stats;

use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};

#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[derive(Parser)]
#[command(name = "dc-tester")]
#[command(about = "dcQUIC load testing tool")]
struct Cli {
    /// Directory to write diagnostic event traces for errored streams
    #[arg(long, default_value = "/tmp/dc-traces")]
    trace_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the RPC server
    Server {
        /// Path to configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Override the server bind address
        #[arg(short, long)]
        address: Option<SocketAddr>,
    },
    /// Start the RPC client
    Client {
        /// Path to configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Server acceptor address to connect to (e.g., [::1]:4433)
        #[arg(short, long)]
        server_addr: Option<SocketAddr>,
    },
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::io::Result<()> {
    init_tracing();

    if let Ok(malloc_conf) = std::env::var("_RJEM_MALLOC_CONF") {
        eprintln!("_RJEM_MALLOC_CONF is set: {}", malloc_conf);
    } else {
        eprintln!("_RJEM_MALLOC_CONF is NOT set");
    }

    // Check if profiling is actually enabled
    match tikv_jemalloc_ctl::profiling::prof::read() {
        Ok(enabled) => eprintln!("jemalloc profiling enabled: {}", enabled),
        Err(e) => eprintln!("jemalloc profiling check failed: {}", e),
    }
    match tikv_jemalloc_ctl::profiling::prof_final::read() {
        Ok(final_dump) => eprintln!("jemalloc prof_final: {}", final_dump),
        Err(e) => eprintln!("jemalloc prof_final check failed: {}", e),
    }
    match tikv_jemalloc_ctl::profiling::lg_prof_interval::read() {
        Ok(interval) => eprintln!(
            "jemalloc lg_prof_interval: {} ({}MB)",
            interval,
            1 << (interval.max(0) - 20)
        ),
        Err(e) => eprintln!("jemalloc lg_prof_interval check failed: {}", e),
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { config, address } => {
            let mut config = if let Some(path) = config {
                config::Config::load(&path)?.server
            } else {
                config::ServerConfig::default()
            };

            if let Some(addr) = address {
                config.address = addr;
            }

            server::run(config, &cli.trace_dir).await
        }
        Commands::Client {
            config,
            server_addr,
        } => {
            // wait for the server to boot
            tokio::time::sleep(core::time::Duration::from_secs(1)).await;

            let config = if let Some(path) = config {
                config::Config::load(&path)?
            } else {
                config::Config {
                    server: config::ServerConfig::default(),
                    client: config::ClientConfig::default(),
                }
            };

            let (acceptor_addr, handshake_addr) = if let Some(addr) = server_addr {
                let mut handshake = addr;
                handshake.set_port(addr.port() - 1);
                (addr, handshake)
            } else {
                let server_addr = config.server.address;
                (server_addr, config.server.handshake_addr())
            };

            client::run(config.client, acceptor_addr, handshake_addr, &cli.trace_dir).await
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .with_env_var("S2N_LOG")
        .from_env()
        .unwrap();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use clap::{Parser, Subcommand};
use s2n_quic_dc::{
    busy_poll::clock::Timer as BusyPollClock, clock::tokio::Clock as TokioClock, socket::rate::Rate,
};
use std::net::SocketAddr;

mod busy_poll;
mod client;
mod pipeline;
mod psk;
mod server;

#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[derive(Parser)]
#[command(name = "wheel-demo")]
#[command(about = "Wheel and channel adapter demonstration with UDP sockets")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Overall bandwidth limit in Gbps
    #[arg(short, long, default_value = "25.0", global = true)]
    bandwidth: f64,

    /// Per-socket bandwidth limit in Gbps
    #[arg(short = 'p', long, default_value = "5.0", global = true)]
    per_socket_bandwidth: f64,

    /// Number of UDP sockets to use (defaults to CPU count)
    #[arg(short = 'n', long, default_value = "64")]
    sockets: usize,

    /// Packet size in bytes (segment size for GSO)
    #[arg(long, default_value = "8192", global = true)]
    packet_size: u16,

    /// Disable GSO (Generic Segmentation Offload)
    #[arg(long, global = true)]
    disable_gso: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the UDP receiver server
    Server {
        /// Server handshake address (data will use port+1, e.g., [::]:5000 for handshake, [::]:5001 for data)
        #[arg(short, long, default_value = "[::]:5000")]
        address: SocketAddr,
    },
    /// Start the UDP sender client
    Client {
        /// Server handshake address (data will use port+1, e.g., [::1]:5000 for handshake, [::1]:5001 for data)
        #[arg(short, long, default_value = "[::1]:5000")]
        server: SocketAddr,
    },
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::io::Result<()> {
    init_tracing();

    let cli = Cli::parse();

    let num_sockets = cli.sockets;

    // Create shared pipeline infrastructure
    let tokio_clock = TokioClock::default();
    let clock = BusyPollClock::new(tokio_clock);
    let busy_poll = crate::busy_poll::pool();
    let send_pool = s2n_quic_dc::socket::pool::Pool::new(u16::MAX);
    let recv_pool = s2n_quic_dc::socket::pool::Pool::new(u16::MAX);
    let counters = pipeline::CounterRegistry::new();

    let (psk_provider, endpoint_addr, is_server) = match &cli.command {
        Commands::Server { address } => {
            // Address is the handshake address
            // Data address will be port + 1
            let handshake_addr = *address;
            let mut data_addr = *address;
            data_addr.set_port(address.port() + 1);

            let provider = psk::server(handshake_addr)
                .await
                .expect("Failed to create PSK server");
            (
                pipeline::PskProvider::Server(provider),
                data_addr, // Server binds data sockets to data_addr
                true,
            )
        }
        Commands::Client { server } => {
            // Server address is the handshake address
            // Client will derive data address as handshake + 1
            let provider = psk::client().expect("Failed to create PSK client");
            (
                pipeline::PskProvider::Client(provider),
                *server, // Pass handshake address to client
                false,
            )
        }
    };

    let gso = s2n_quic_platform::features::Gso::default();
    if cli.disable_gso {
        gso.disable();
    }

    // Create acceptor registry for flow initialization
    let acceptor_registry = s2n_quic_dc::acceptor::Registry::new();

    let config = pipeline::PipelineConfig {
        packet_size: cli.packet_size,
        overall_send_rate: Rate::new(cli.bandwidth),
        per_socket_send_rate: Rate::new(cli.per_socket_bandwidth),
        busy_poll: &busy_poll,
        clock,
        send_pool,
        recv_pool,
        counters,
        psk_provider,
        gso,
        acceptor_registry,
    };

    if is_server {
        server::run(endpoint_addr, num_sockets, config).await
    } else {
        client::run(endpoint_addr, num_sockets, config).await
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

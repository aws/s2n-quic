// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use aya::{
    maps::{HashMap, MapData, XskMap},
    programs, Ebpf,
};
use s2n_quic::provider::io::{
    self,
    xdp::{
        bpf, encoder,
        if_xdp::{self, XdpFlags},
        io::{
            self as xdp_io,
            rx::{Driver as _, WithCooldown},
        },
        ring, socket, syscall,
        tx::{self, TxExt as _},
        umem, Provider,
    },
};
use s2n_quic_core::task::cooldown::Cooldown;
use std::{ffi::CString, net::SocketAddr, os::unix::io::AsRawFd, sync::Arc};
use structopt::StructOpt;
use tokio::{io::unix::AsyncFd, net::UdpSocket};

#[derive(Debug, StructOpt)]
pub struct Xdp {
    #[structopt(long, default_value = "lo")]
    interface: String,

    // Default values come from https://elixir.bootlin.com/linux/v6.3.9/source/tools/testing/selftests/bpf/xsk.h#L185
    #[structopt(long, default_value = "2048")]
    tx_queue_len: u32,

    #[structopt(long, default_value = "2048")]
    rx_queue_len: u32,

    #[structopt(long, default_value = "4096")]
    frame_size: u32,

    #[structopt(long)]
    xdp_stats: bool,

    #[structopt(long)]
    bpf_trace: bool,

    #[structopt(long, default_value = "auto")]
    xdp_mode: XdpMode,

    #[structopt(long)]
    no_checksum: bool,

    #[structopt(long, default_value)]
    rx_cooldown: u16,
}

#[derive(Clone, Copy, Debug)]
enum XdpMode {
    /// Automatically selects an XDP mode based on the capabilities of the NIC
    Auto,
    /// Uses the software SKB (socket buffer) mode - usually requires no NIC support
    Skb,
    /// Uses the driver mode, which integrates with XDP directly in the kernel driver
    Drv,
    /// Uses the hardware mode, which integrates with XDP directly in the actual NIC hardware
    Hw,
}

impl core::str::FromStr for XdpMode {
    type Err = crate::Error;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        Ok(match v {
            "auto" => Self::Auto,
            "skb" => Self::Skb,
            "drv" | "driver" => Self::Drv,
            "hw" | "hardware" => Self::Hw,
            _ => return Err(format!("invalid xdp-mode: {v:?}").into()),
        })
    }
}

impl From<XdpMode> for programs::xdp::XdpFlags {
    fn from(mode: XdpMode) -> Self {
        match mode {
            XdpMode::Auto => Self::default(),
            XdpMode::Skb => Self::SKB_MODE,
            XdpMode::Drv => Self::DRV_MODE,
            XdpMode::Hw => Self::HW_MODE,
        }
    }
}

type SetupResult = Result<(
    umem::Umem,
    Vec<xdp_io::rx::Channel<WithCooldown<Arc<AsyncFd<socket::Fd>>>>>,
    Vec<(u32, socket::Fd)>,
    Vec<xdp_io::tx::Channel<xdp_io::tx::BusyPoll>>,
)>;

impl Xdp {
    fn setup(&self) -> SetupResult {
        let frame_size = self.frame_size;
        let rx_queue_len = self.rx_queue_len;
        let tx_queue_len = self.tx_queue_len;
        let fill_ring_len = rx_queue_len * 2;
        let completion_ring_len = tx_queue_len;

        let max_queues = syscall::max_queues(&self.interface);
        let umem_size = (rx_queue_len + tx_queue_len) * max_queues;

        // create a UMEM
        let umem = umem::Builder {
            frame_count: umem_size,
            frame_size,
            ..Default::default()
        }
        .build()?;

        // setup the address we're going to bind to
        let mut address = if_xdp::Address {
            flags: XdpFlags::USE_NEED_WAKEUP,
            ..Default::default()
        };
        address.set_if_name(&CString::new(self.interface.clone())?)?;

        let mut shared_umem_fd = None;
        let mut tx_channels = vec![];
        let mut rx_channels = vec![];
        let mut rx_fds = vec![];

        let mut desc = umem.frames();

        // iterate over all of the queues and create sockets for each one
        for queue_id in 0..max_queues {
            let socket = socket::Fd::open()?;

            // if we've already attached a socket to the UMEM, then reuse the first FD
            if let Some(fd) = shared_umem_fd {
                address.set_shared_umem(&fd);
            } else {
                socket.attach_umem(&umem)?;
                shared_umem_fd = Some(socket.as_raw_fd());
            }

            // set the queue id to the current value
            address.queue_id = queue_id;

            // file descriptors can only be added once so wrap it in an Arc
            let async_fd = Arc::new(AsyncFd::new(socket.clone())?);

            // get the offsets for each of the rings
            let offsets = syscall::offsets(&socket)?;

            {
                // create a pair of rings for receiving packets
                let mut fill = ring::Fill::new(socket.clone(), &offsets, fill_ring_len)?;
                let rx = ring::Rx::new(socket.clone(), &offsets, rx_queue_len)?;

                // remember the FD so we can add it to the XSK map later
                rx_fds.push((queue_id, socket.clone()));

                // put descriptors in the Fill queue
                fill.init((&mut desc).take(rx_queue_len as _));

                let cooldown = Cooldown::new(self.rx_cooldown);

                rx_channels.push(xdp_io::rx::Channel {
                    rx,
                    fill,
                    driver: async_fd.clone().with_cooldown(cooldown),
                });
            };

            {
                // create a pair of rings for transmitting packets
                let mut completion =
                    ring::Completion::new(socket.clone(), &offsets, completion_ring_len)?;
                let tx = ring::Tx::new(socket.clone(), &offsets, tx_queue_len)?;

                // put descriptors in the completion queue
                completion.init((&mut desc).take(tx_queue_len as _));

                tx_channels.push(xdp_io::tx::Channel {
                    tx,
                    completion,
                    driver: xdp_io::tx::BusyPoll,
                });
            };

            // finally bind the socket to the configured address
            syscall::bind(&socket, &mut address)?;
        }

        // make sure we've allocated all descriptors from the UMEM to a queue
        assert_eq!(desc.count(), 0, "descriptors have been leaked");

        Ok((umem, rx_channels, rx_fds, tx_channels))
    }

    fn bpf_task(&self, port: u16, rx_fds: Vec<(u32, socket::Fd)>) -> Result<()> {
        // load the default BPF program from s2n-quic-xdp
        let mut bpf = if self.bpf_trace {
            let mut bpf = Ebpf::load(bpf::DEFAULT_PROGRAM_TRACE)?;

            if let Err(err) = aya_log::EbpfLogger::init(&mut bpf) {
                eprint!("error initializing BPF trace: {err:?}");
            }

            bpf
        } else {
            Ebpf::load(bpf::DEFAULT_PROGRAM)?
        };

        let interface = self.interface.clone();
        let xdp_stats = self.xdp_stats;
        let xdp_mode = self.xdp_mode.into();

        let program: &mut programs::Xdp = bpf
            .program_mut(bpf::PROGRAM_NAME)
            .expect("missing default program")
            .try_into()?;
        program.load()?;

        // attach the BPF program to the interface
        let link_id = program.attach(&interface, xdp_mode)?;

        let bpf_task = async move {
            // register the port as active
            let mut ports: HashMap<_, _, _> = bpf
                .map_mut(bpf::PORT_MAP_NAME)
                .expect("missing port map")
                .try_into()?;

            // the BPF program just needs to have a non-zero value for the port
            let enabled = 1u8;
            // no flags are needed
            let flags = 0;
            ports.insert(port, enabled, flags)?;

            // register all of the RX sockets on each of the queues
            let mut xskmap: XskMap<&mut MapData> = bpf
                .map_mut(bpf::XSK_MAP_NAME)
                .expect("missing socket map")
                .try_into()?;

            for (queue_id, socket) in &rx_fds {
                xskmap.set(*queue_id, socket.as_raw_fd(), 0)?;
            }

            // print xdp stats every second, if configured
            if xdp_stats {
                loop {
                    tokio::time::sleep(core::time::Duration::from_secs(1)).await;
                    for (queue_id, socket) in &rx_fds {
                        if let Ok(stats) = syscall::statistics(socket) {
                            println!("stats[{queue_id}]: {stats:?}");
                        }
                    }
                }
            }

            // we want this future to go until the end of the program so we can keep the BPF
            // program active on the the NIC.
            core::future::pending::<()>().await;

            // retain the bpf program for the duration of execution
            let _ = bpf;
            let _ = link_id;
            let _ = rx_fds;

            Result::<(), crate::Error>::Ok(())
        };

        tokio::spawn(async move {
            if let Err(error) = bpf_task.await {
                panic!("BPF ERROR: {error}");
            }
        });

        Ok(())
    }

    fn udp_socket(&self, addr: SocketAddr) -> Result<std::net::UdpSocket> {
        let interface = CString::new(self.interface.clone())?;

        let udp_socket = socket::bind_udp(&interface, addr)?;

        Ok(udp_socket)
    }

    fn tx_encoder(&self) -> encoder::Config {
        let mut encoder = encoder::Config::default();
        if self.no_checksum {
            encoder.set_checksum(false);
        }
        encoder
    }

    fn spawn_udp_rx(&self, udp_socket: UdpSocket) {
        // Set up a task to read from the bound UDP socket
        //
        // If everything is working properly, this won't ever get a packet, since the AF_XDP socket
        // is intercepting all packets on this port. If it does start logging, something has gone
        // wrong with the BPF setup.
        let mut recv_buffer = vec![0; self.frame_size as usize];
        tokio::spawn(async move {
            let result = udp_socket.recv_from(&mut recv_buffer).await;
            eprintln!("ERROR: received packet on regular UDP socket: {result:?}");

            std::process::exit(1);
        });
    }

    pub fn server(&self, addr: SocketAddr) -> Result<impl io::Provider> {
        let udp_socket = self.udp_socket(addr)?;
        let udp_socket = UdpSocket::from_std(udp_socket)?;

        let (umem, rx, rx_fds, tx) = self.setup()?;

        self.bpf_task(addr.port(), rx_fds)?;

        let io_rx = xdp_io::rx::Rx::new(rx, umem.clone());
        let io_tx = xdp_io::tx::Tx::new(tx, umem, self.tx_encoder());

        let provider = Provider::builder()
            .with_rx(io_rx)
            .with_tx(io_tx)
            .with_frame_size(self.frame_size as _)?
            .build();

        self.spawn_udp_rx(udp_socket);

        Ok(provider)
    }

    pub fn client(&self, addr: SocketAddr) -> Result<impl io::Provider> {
        let udp_socket = self.udp_socket(addr)?;
        // query the actual port that was selected for the socket in case it was `0`
        let addr = udp_socket.local_addr()?;

        let recv_udp_socket = udp_socket.try_clone();

        let (umem, rx, rx_fds, tx) = self.setup()?;

        self.bpf_task(addr.port(), rx_fds)?;

        let (stats_sender, stats_recv) = socket::stats::channel();

        let io_rx = xdp_io::rx::Rx::new(rx, umem.clone());

        let io_tx = {
            let tx = xdp_io::tx::Tx::new(tx, umem, self.tx_encoder());

            let udp_tx = {
                let (udp_tx, udp_task) = tx::channel(udp_socket, stats_sender);

                tokio::spawn(udp_task);

                udp_tx.with_handle_map(|handle: &io::xdp::PathHandle| {
                    // convert the XDP handle into the regular UDP handle
                    handle.into()
                })
            };

            // route any initial packets to the UDP socket so we can offload address resolution on
            // the OS
            tx.with_router(xdp_io::router::Router::default(), udp_tx)
        };

        let provider = Provider::builder()
            .with_rx(io_rx)
            .with_tx(io_tx)
            .with_frame_size(self.frame_size as _)?
            .with_stats(stats_recv)
            .build();

        if let Ok(udp_socket) = recv_udp_socket {
            let udp_socket = UdpSocket::from_std(udp_socket)?;
            self.spawn_udp_rx(udp_socket);
        }

        Ok(provider)
    }
}

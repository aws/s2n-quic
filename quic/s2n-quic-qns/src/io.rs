// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use s2n_quic::provider::io;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Server {
    #[structopt(short, long, default_value = "::")]
    pub ip: std::net::IpAddr,

    #[structopt(short, long, default_value = "443")]
    pub port: u16,

    #[structopt(long)]
    pub disable_gso: bool,

    #[structopt(long, default_value = "1280")]
    pub initial_mtu: u16,

    #[structopt(long, default_value = "9000")]
    pub max_mtu: u16,

    #[structopt(long)]
    pub queue_recv_buffer_size: Option<usize>,

    #[structopt(long)]
    pub queue_send_buffer_size: Option<usize>,

    #[cfg(feature = "xdp")]
    #[structopt(flatten)]
    xdp: crate::xdp::Xdp,
}

impl Server {
    #[cfg(feature = "xdp")]
    pub fn build(&self) -> Result<impl io::Provider> {
        // GSO isn't currently supported for XDP so just read it to avoid a dead_code warning
        let _ = self.disable_gso;
        let _ = self.max_mtu;

        let addr = (self.ip, self.port).into();

        self.xdp.server(addr)
    }

    #[cfg(not(feature = "xdp"))]
    pub fn build(&self) -> Result<impl io::Provider> {
        let mut io_builder = io::Default::builder()
            .with_receive_address((self.ip, self.port).into())?
            .with_initial_mtu(self.initial_mtu)?
            .with_max_mtu(self.max_mtu)?
            .with_gso(!self.disable_gso)?;

        if let Some(size) = self.queue_send_buffer_size {
            io_builder = io_builder.with_internal_send_buffer_size(size)?;
        }

        if let Some(size) = self.queue_recv_buffer_size {
            io_builder = io_builder.with_internal_recv_buffer_size(size)?;
        }

        Ok(io_builder.build()?)
    }
}

#[derive(Debug, StructOpt)]
pub struct Client {
    #[structopt(long)]
    pub disable_gso: bool,

    #[structopt(long, default_value = "1280")]
    pub initial_mtu: u16,

    #[structopt(long, default_value = "9000")]
    pub max_mtu: u16,

    #[structopt(long)]
    pub queue_recv_buffer_size: Option<usize>,

    #[structopt(long)]
    pub queue_send_buffer_size: Option<usize>,

    #[structopt(short, long, default_value = "::")]
    pub local_ip: std::net::IpAddr,

    #[cfg(feature = "xdp")]
    #[structopt(flatten)]
    xdp: crate::xdp::Xdp,
}

impl Client {
    #[cfg(feature = "xdp")]
    pub fn build(&self) -> Result<impl io::Provider> {
        // GSO isn't currently supported for XDP so just read it to avoid a dead_code warning
        let _ = self.disable_gso;
        let _ = self.max_mtu;

        let addr = (self.local_ip, 0u16).into();

        self.xdp.client(addr)
    }

    #[cfg(not(feature = "xdp"))]
    pub fn build(&self) -> Result<impl io::Provider> {
        let mut io_builder = io::Default::builder()
            .with_receive_address((self.local_ip, 0u16).into())?
            .with_initial_mtu(self.initial_mtu)?
            .with_max_mtu(self.max_mtu)?
            .with_gso(!self.disable_gso)?;

        if let Some(size) = self.queue_send_buffer_size {
            io_builder = io_builder.with_internal_send_buffer_size(size)?;
        }

        if let Some(size) = self.queue_recv_buffer_size {
            io_builder = io_builder.with_internal_recv_buffer_size(size)?;
        }

        Ok(io_builder.build()?)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod busy_poll;
pub mod cached_addr;
pub mod channel;
pub mod fd;
mod gso;
pub mod pool;
pub mod rate;
pub mod recv;
pub mod send;
pub mod tracing;

pub use busy_poll::BusyPoll;
pub use gso::Gso;
pub use s2n_quic_platform::socket::options::{Options, ReusePort};

pub trait LocalAddr {
    fn local_addr(&self) -> std::io::Result<std::net::SocketAddr>;
}

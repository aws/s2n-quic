//! This module contains abstractions around the platform on which the
//! stack is running

#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

#[macro_use]
pub mod socket;

pub mod buffer;
pub mod io;
pub mod message;
pub mod time;

pub mod default {
    use crate::{buffer::default as buffer, io::default as io, socket::default as socket};

    pub type Buffer = buffer::Buffer;
    pub type Rx = io::rx::Rx<buffer::Buffer, socket::Socket>;
    pub type Tx = io::tx::Tx<buffer::Buffer, socket::Socket>;
    pub type Duplex = s2n_quic_core::io::Duplex<Rx, Tx>;
    pub type Socket = socket::Socket;
}

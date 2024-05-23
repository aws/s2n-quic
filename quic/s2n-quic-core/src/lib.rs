// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[macro_use]
#[doc(hidden)]
pub mod macros;

#[macro_use]
pub mod probe;

pub mod ack;
pub mod application;
#[cfg(feature = "alloc")]
pub mod buffer;
pub mod connection;
pub mod counter;
pub mod crypto;
pub mod ct;
pub mod datagram;
#[cfg(feature = "alloc")]
pub mod dc;
pub mod endpoint;
pub mod event;
pub mod frame;
pub mod havoc;
pub mod inet;
#[cfg(feature = "alloc")]
pub mod interval_set;
pub mod io;
pub mod memo;
pub mod number;
pub mod packet;
pub mod path;
pub mod query;
pub mod random;
pub mod recovery;
pub mod slice;
pub mod state;
pub mod stateless_reset;
pub mod stream;
pub mod sync;
pub mod task;
pub mod time;
pub mod token;
pub mod transmission;
pub mod transport;
pub mod varint;
pub mod xdp;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

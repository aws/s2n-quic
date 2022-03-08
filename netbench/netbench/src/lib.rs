// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub type Result<T, E = Error> = core::result::Result<T, E>;
pub type Error = Box<dyn std::error::Error>;

mod checkpoints;
pub mod client;
mod connection;
mod driver;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub mod helper;
pub mod multiplex;
pub mod operation;
#[cfg(feature = "s2n-quic")]
pub mod s2n_quic;
pub mod scenario;
pub mod timer;
pub mod trace;
pub mod units;

pub use checkpoints::Checkpoints;
pub use client::Driver as Client;
pub use connection::Connection;
pub use driver::Driver;
pub use timer::Timer;
pub use trace::Trace;

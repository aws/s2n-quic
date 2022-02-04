// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub type Result<T, E = Error> = core::result::Result<T, E>;
pub type Error = Box<dyn std::error::Error>;

mod checkpoints;
mod connection;
mod driver;
pub mod helper;
pub mod multiplex;
pub mod operation;
#[cfg(feature = "s2n-quic")]
pub mod s2n_quic;
pub mod scenario;
pub mod trace;
pub mod units;

pub use checkpoints::Checkpoints;
pub use connection::Connection;
pub use driver::Driver;
pub use trace::Trace;

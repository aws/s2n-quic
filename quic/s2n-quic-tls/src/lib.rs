// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Ensure memory is correctly managed in tests
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: checkers::Allocator = checkers::Allocator::system();

mod callback;
mod keylog;
mod params;
mod session;

pub mod certificate;
pub mod client;
pub mod server;
pub mod verify_host;

pub use client::Client;
pub use server::Server;

#[cfg(test)]
mod tests;

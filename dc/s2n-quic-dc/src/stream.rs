// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod client;
mod coop;
mod reader;
pub mod server;
mod stream;
mod writer;

pub use crate::endpoint::Error;
pub use client::Client;
pub use reader::Reader;
pub use server::Server;
pub use stream::{PendingValidation, Stream};
pub use writer::Writer;

#[deprecated = "use crate::endpoint instead"]
pub use crate::endpoint;

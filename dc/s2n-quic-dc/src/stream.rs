// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod client;
mod coop;
pub(crate) mod metrics;
mod reader;
pub mod server;
mod stream;
mod writer;

pub use crate::{credit::Priority, endpoint::Error};
pub use client::Client;
pub use reader::Reader;
pub use server::Server;
pub use stream::Stream;
pub use writer::{MsgFlags, Writer};

#[deprecated = "use crate::endpoint instead"]
pub use crate::endpoint;

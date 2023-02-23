// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod error;
#[cfg(feature = "alloc")]
mod server_name;

pub use error::Error;
#[cfg(feature = "alloc")]
pub use server_name::ServerName;

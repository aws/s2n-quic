// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
pub mod client;
pub mod file;
pub mod h3;
pub mod server;

pub type Error = Box<dyn 'static + std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = core::result::Result<T, E>;

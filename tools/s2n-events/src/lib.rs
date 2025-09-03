// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod output;
mod output_config;

pub mod parser;
pub mod validation;

pub use output::Output;
pub use output_config::{OutputConfig, OutputMode};

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = core::result::Result<T, E>;

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod generate_config;
mod output;

pub mod parser;
pub mod validation;

pub use generate_config::{GenerateConfig, OutputCApi, OutputMode};
pub use output::Output;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = core::result::Result<T, E>;

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod output;
mod output_mode;

pub mod parser;
pub mod validation;

pub use output::Output;
pub use output_mode::OutputMode;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = core::result::Result<T, E>;

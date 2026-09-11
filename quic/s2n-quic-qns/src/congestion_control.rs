// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use clap::{builder::TypedValueParser as _, Args};
use core::str::FromStr;
use std::io;

#[derive(Debug, Args)]
pub struct CongestionControl {
    /// The congestion controller to use
    #[clap(
        long = "cc",
        default_value = "bbr",
        value_parser = clap::builder::PossibleValuesParser::new(["cubic", "bbr"])
            .map(|s| s.parse::<CongestionController>().unwrap()),
    )]
    pub congestion_controller: CongestionController,
}

#[derive(Copy, Clone, Debug)]
pub enum CongestionController {
    Cubic,
    Bbr,
}

impl FromStr for CongestionController {
    type Err = crate::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cubic" => Ok(Self::Cubic),
            "bbr" => Ok(Self::Bbr),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Unsupported congestion controller: {s}"),
            )
            .into()),
        }
    }
}

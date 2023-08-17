// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::str::FromStr;
use std::io;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct CongestionControl {
    /// The congestion controller to use
    #[structopt(long = "cc", default_value = "bbr", possible_values = &["cubic","bbr"])]
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

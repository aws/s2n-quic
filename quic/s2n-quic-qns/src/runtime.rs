// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use structopt::StructOpt;
use tokio::runtime::Runtime as Rt;

#[derive(Clone, Debug, StructOpt)]
pub struct Runtime {
    /// Enables the multi-threaded runtime
    #[structopt(long)]
    multithread: Option<Option<bool>>,
}

impl Runtime {
    pub fn build(&self) -> Result<Rt> {
        let runtime = if self.multithread() {
            tokio::runtime::Builder::new_multi_thread()
        } else {
            tokio::runtime::Builder::new_current_thread()
        }
        .enable_all()
        .build()?;
        Ok(runtime)
    }

    pub fn multithread(&self) -> bool {
        match self.multithread {
            Some(Some(v)) => v,
            Some(None) => true,
            None => false,
        }
    }
}

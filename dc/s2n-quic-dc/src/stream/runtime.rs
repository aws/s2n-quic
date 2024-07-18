// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{recv, send};
use std::sync::Arc;

#[cfg(feature = "tokio")]
pub mod tokio;

pub type ArcHandle = Arc<dyn Handle>;

pub trait Handle: 'static + Send + Sync {
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown);
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown);
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{recv, send},
};
use std::sync::Arc;

#[cfg(feature = "tokio")]
pub mod tokio;

pub type ArcHandle<Sub> = Arc<dyn Handle<Sub>>;

pub trait Handle<Sub>: 'static + Send + Sync
where
    Sub: event::Subscriber,
{
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown<Sub>);
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown<Sub>);
}

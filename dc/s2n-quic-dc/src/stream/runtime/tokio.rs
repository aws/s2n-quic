// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{recv, send};
use core::{mem::MaybeUninit, ops};
use std::sync::Arc;

impl super::Handle for tokio::runtime::Handle {
    #[inline]
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown) {
        self.spawn(async move {
            // Note: Must be created inside spawn() since ambient runtime is otherwise not
            // guaranteed and will cause a panic on the timeout future construction.
            // make sure the task doesn't hang around indefinitely
            tokio::time::timeout(core::time::Duration::from_secs(1), shutdown).await
        });
    }

    #[inline]
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown) {
        self.spawn(async move {
            // Note: Must be created inside spawn() since ambient runtime is otherwise not
            // guaranteed and will cause a panic on the timeout future construction.
            // make sure the task doesn't hang around indefinitely
            tokio::time::timeout(core::time::Duration::from_secs(1), shutdown).await
        });
    }
}

#[derive(Clone)]
pub struct Shared(Arc<SharedInner>);

impl Shared {
    #[inline]
    pub fn handle(&self) -> super::ArcHandle {
        self.0.clone()
    }
}

impl ops::Deref for Shared {
    type Target = tokio::runtime::Handle;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<tokio::runtime::Runtime> for Shared {
    fn from(rt: tokio::runtime::Runtime) -> Self {
        Self(Arc::new(SharedInner(MaybeUninit::new(rt))))
    }
}

struct SharedInner(MaybeUninit<tokio::runtime::Runtime>);

impl ops::Deref for SharedInner {
    type Target = tokio::runtime::Handle;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { (self.0).assume_init_ref().handle() }
    }
}

impl super::Handle for SharedInner {
    #[inline]
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown) {
        (**self).spawn_recv_shutdown(shutdown)
    }

    #[inline]
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown) {
        (**self).spawn_send_shutdown(shutdown)
    }
}

impl Drop for SharedInner {
    fn drop(&mut self) {
        // drop the runtimes in a separate thread to avoid tokio complaining
        let rt = unsafe { self.0.assume_init_read() };
        std::thread::spawn(move || {
            // give enough time for all of the streams to shut down
            rt.shutdown_timeout(core::time::Duration::from_secs(10));
        });
    }
}

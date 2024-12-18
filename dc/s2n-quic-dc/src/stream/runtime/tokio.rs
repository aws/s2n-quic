// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{recv, send},
};
use core::{marker::PhantomData, mem::MaybeUninit, ops};
use std::sync::Arc;

impl<Sub> super::Handle<Sub> for tokio::runtime::Handle
where
    Sub: event::Subscriber,
{
    #[inline]
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown<Sub>) {
        self.spawn(async move {
            // Note: Must be created inside spawn() since ambient runtime is otherwise not
            // guaranteed and will cause a panic on the timeout future construction.
            // make sure the task doesn't hang around indefinitely
            tokio::time::timeout(core::time::Duration::from_secs(1), shutdown).await
        });
    }

    #[inline]
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown<Sub>) {
        self.spawn(async move {
            // Note: Must be created inside spawn() since ambient runtime is otherwise not
            // guaranteed and will cause a panic on the timeout future construction.
            // make sure the task doesn't hang around indefinitely
            tokio::time::timeout(core::time::Duration::from_secs(1), shutdown).await
        });
    }
}

#[derive(Clone)]
pub struct Shared<Sub>(Arc<SharedInner<Sub>>);

impl<Sub> Shared<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn handle(&self) -> super::ArcHandle<Sub> {
        self.0.clone()
    }
}

impl<Sub> ops::Deref for Shared<Sub>
where
    Sub: event::Subscriber,
{
    type Target = tokio::runtime::Handle;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<Sub> From<tokio::runtime::Runtime> for Shared<Sub>
where
    Sub: event::Subscriber,
{
    fn from(rt: tokio::runtime::Runtime) -> Self {
        let runtime = MaybeUninit::new(rt);
        Self(Arc::new(SharedInner {
            runtime,
            sub: PhantomData,
        }))
    }
}

struct SharedInner<Sub> {
    runtime: MaybeUninit<tokio::runtime::Runtime>,
    sub: PhantomData<Sub>,
}

impl<Sub> ops::Deref for SharedInner<Sub> {
    type Target = tokio::runtime::Handle;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { self.runtime.assume_init_ref().handle() }
    }
}

impl<Sub> super::Handle<Sub> for SharedInner<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown<Sub>) {
        (**self).spawn_recv_shutdown(shutdown)
    }

    #[inline]
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown<Sub>) {
        (**self).spawn_send_shutdown(shutdown)
    }
}

impl<Sub> Drop for SharedInner<Sub> {
    fn drop(&mut self) {
        // drop the runtimes in a separate thread to avoid tokio complaining
        let rt = unsafe { self.runtime.assume_init_read() };
        std::thread::spawn(move || {
            // give enough time for all of the streams to shut down
            rt.shutdown_timeout(core::time::Duration::from_secs(10));
        });
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{recv, send},
};
use bach::{ext::*, time::timeout};

pub use bach::runtime::Handle;

impl<Sub> super::Handle<Sub> for Handle
where
    Sub: event::Subscriber,
{
    #[inline]
    fn spawn_recv_shutdown(&self, shutdown: recv::application::Shutdown<Sub>) {
        self.spawn(
            async move {
                // Note: Must be created inside spawn() since ambient runtime is otherwise not
                // guaranteed and will cause a panic on the timeout future construction.
                // make sure the task doesn't hang around indefinitely
                timeout(core::time::Duration::from_secs(1), shutdown).await
            }
            .primary(),
        );
    }

    #[inline]
    fn spawn_send_shutdown(&self, shutdown: send::application::Shutdown<Sub>) {
        self.spawn(
            async move {
                // Note: Must be created inside spawn() since ambient runtime is otherwise not
                // guaranteed and will cause a panic on the timeout future construction.
                // make sure the task doesn't hang around indefinitely
                timeout(core::time::Duration::from_secs(1), shutdown).await
            }
            .primary(),
        );
    }
}

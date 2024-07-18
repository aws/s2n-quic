// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{
    runtime,
    send::application::{Inner, Writer},
    shared::ArcShared,
    socket,
};

pub struct Builder {
    runtime: runtime::ArcHandle,
}

impl Builder {
    #[inline]
    pub fn new(runtime: runtime::ArcHandle) -> Self {
        Self { runtime }
    }

    #[inline]
    pub fn build(self, shared: ArcShared, sockets: socket::ArcApplication) -> Writer {
        let Self { runtime } = self;
        Writer(Box::new(Inner {
            shared,
            sockets,
            queue: Default::default(),
            pacer: Default::default(),
            open: true,
            runtime,
        }))
    }
}

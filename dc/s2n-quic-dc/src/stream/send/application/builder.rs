// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        runtime,
        send::application::{Inner, Writer},
        shared::ArcShared,
        socket,
        transport::Transport,
    },
};

pub struct Builder<Sub>
where
    Sub: event::Subscriber,
{
    runtime: runtime::ArcHandle<Sub>,
}

impl<Sub> Builder<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn new(runtime: runtime::ArcHandle<Sub>) -> Self {
        Self { runtime }
    }

    #[inline]
    pub fn build(self, shared: ArcShared<Sub>, sockets: socket::ArcApplication) -> Writer<Sub> {
        let Self { runtime } = self;
        let is_tls = shared.common.s2n_connection.is_some();
        let transport = Transport::variant(&*sockets, is_tls);

        Writer(Box::new(transport(Inner {
            shared,
            sockets,
            queue: Default::default(),
            pacer: Default::default(),
            status: Default::default(),
            runtime,
        })))
    }
}

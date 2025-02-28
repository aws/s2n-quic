// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::Timer,
    event, msg,
    stream::{
        recv::application::{Inner, LocalState, Reader},
        runtime,
        shared::ArcShared,
        socket,
    },
};
use core::mem::ManuallyDrop;
use s2n_quic_core::endpoint;

pub struct Builder<Sub> {
    endpoint: endpoint::Type,
    runtime: runtime::ArcHandle<Sub>,
}

impl<Sub> Builder<Sub> {
    #[inline]
    pub fn new(endpoint: endpoint::Type, runtime: runtime::ArcHandle<Sub>) -> Self {
        Self { endpoint, runtime }
    }
}

impl<Sub> Builder<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn build(self, shared: ArcShared<Sub>, sockets: socket::ArcApplication) -> Reader<Sub> {
        let Self { endpoint, runtime } = self;

        let remote_addr = shared.read_remote_addr();
        // we only need a timer for unreliable transports
        let is_reliable = sockets.features().is_reliable();
        let timer = if is_reliable {
            None
        } else {
            Some(Timer::new(&shared.clock))
        };
        let gso = shared.gso.clone();
        let send_buffer = msg::send::Message::new(remote_addr, gso);
        let read_mode = Default::default();
        let ack_mode = Default::default();
        let local_state = match endpoint {
            // reliable transports on the client need to read at least one packet in order to
            // process secret control packets
            endpoint::Type::Client if is_reliable => LocalState::Ready,
            // unreliable transports use background workers to drive state
            endpoint::Type::Client => LocalState::Reading,
            // the server acceptor already read from the socket at least once
            endpoint::Type::Server => LocalState::Reading,
        };

        Reader(ManuallyDrop::new(Box::new(Inner {
            shared,
            sockets,
            send_buffer,
            read_mode,
            ack_mode,
            timer,
            local_state,
            runtime,
        })))
    }
}

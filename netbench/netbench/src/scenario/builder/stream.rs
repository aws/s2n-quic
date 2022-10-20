// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::connection;
use crate::{
    operation as op,
    units::{Byte, Rate},
};
use core::marker::PhantomData;

macro_rules! send_stream {
    () => {
        pub fn send(&mut self, bytes: Byte) -> &mut Self {
            self.ops.push(crate::operation::Connection::Send {
                stream_id: self.id,
                bytes,
            });
            self
        }

        pub fn set_send_rate(&mut self, rate: Rate) -> &mut Self {
            self.ops.push(crate::operation::Connection::SendRate {
                stream_id: self.id,
                rate,
            });
            self
        }
    };
}

macro_rules! receive_stream {
    () => {
        pub fn receive(&mut self, bytes: Byte) -> &mut Self {
            self.ops.push(crate::operation::Connection::Receive {
                stream_id: self.id,
                bytes,
            });
            self
        }

        pub fn set_receive_rate(&mut self, rate: Rate) -> &mut Self {
            self.ops.push(crate::operation::Connection::ReceiveRate {
                stream_id: self.id,
                rate,
            });
            self
        }

        pub fn receive_all(&mut self) -> &mut Self {
            self.ops
                .push(crate::operation::Connection::ReceiveAll { stream_id: self.id });
            self
        }
    };
}

pub struct Stream<Endpoint, Location> {
    id: u64,
    ops: Vec<op::Connection>,
    state: connection::State,
    endpoint: PhantomData<Endpoint>,
    location: PhantomData<Location>,
}

impl<Endpoint, Location> Stream<Endpoint, Location> {
    send_stream!();
    receive_stream!();
    sync!(Endpoint, Location);
    sleep!();
    trace!();
    iterate!();

    pub(crate) fn new(id: u64, state: connection::State) -> Self {
        Self {
            id,
            ops: vec![],
            state,
            endpoint: PhantomData,
            location: PhantomData,
        }
    }

    fn child_scope(&self) -> Self {
        Self::new(self.id, self.state.clone())
    }

    fn finish_scope(self) -> Vec<op::Connection> {
        self.ops
    }

    pub fn concurrently<
        S: FnOnce(&mut SendStream<Endpoint, Location>),
        R: FnOnce(&mut ReceiveStream<Endpoint, Location>),
    >(
        &mut self,
        send: S,
        receive: R,
    ) -> &mut Self {
        let mut send_stream = SendStream::new(self.id, self.state.clone());
        let mut receive_stream = ReceiveStream::new(self.id, self.state.clone());
        send(&mut send_stream);
        receive(&mut receive_stream);
        let threads = vec![send_stream.ops, receive_stream.ops];
        self.ops.push(op::Connection::Scope { threads });
        self
    }

    pub(crate) fn finish(mut self) -> Vec<op::Connection> {
        let stream_id = self.id;
        self.ops.push(op::Connection::SendFinish { stream_id });
        self.ops.push(op::Connection::ReceiveFinish { stream_id });
        self.ops
    }
}

pub struct SendStream<Endpoint, Location> {
    id: u64,
    ops: Vec<op::Connection>,
    state: connection::State,
    endpoint: PhantomData<Endpoint>,
    location: PhantomData<Location>,
}

impl<Endpoint, Location> SendStream<Endpoint, Location> {
    send_stream!();
    sync!(Endpoint, Location);
    sleep!();
    trace!();
    iterate!();

    pub(crate) fn new(id: u64, state: connection::State) -> Self {
        Self {
            id,
            ops: vec![],
            state,
            endpoint: PhantomData,
            location: PhantomData,
        }
    }

    fn child_scope(&self) -> Self {
        Self::new(self.id, self.state.clone())
    }

    fn finish_scope(self) -> Vec<op::Connection> {
        self.ops
    }

    pub(crate) fn finish(mut self) -> Vec<op::Connection> {
        let stream_id = self.id;
        self.ops.push(op::Connection::SendFinish { stream_id });
        self.ops
    }
}

pub struct ReceiveStream<Endpoint, Location> {
    id: u64,
    ops: Vec<op::Connection>,
    state: connection::State,
    endpoint: PhantomData<Endpoint>,
    location: PhantomData<Location>,
}

impl<Endpoint, Location> ReceiveStream<Endpoint, Location> {
    receive_stream!();
    sync!(Endpoint, Location);
    sleep!();
    trace!();
    iterate!();

    pub(crate) fn new(id: u64, state: connection::State) -> Self {
        Self {
            id,
            ops: vec![],
            state,
            endpoint: PhantomData,
            location: PhantomData,
        }
    }

    fn child_scope(&self) -> Self {
        Self::new(self.id, self.state.clone())
    }

    fn finish_scope(self) -> Vec<op::Connection> {
        self.ops
    }

    pub(crate) fn finish(mut self) -> Vec<op::Connection> {
        let stream_id = self.id;
        self.ops.push(op::Connection::ReceiveFinish { stream_id });
        self.ops
    }
}

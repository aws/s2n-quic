// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::{
    convert::TryInto,
    task::{Context, Poll},
};
use s2n_quic_core::{
    endpoint::{self, CloseError},
    event,
    inet::SocketAddress,
    io::{rx, tx},
    path::Handle as _,
    time::{timer::Provider as _, Clock, Duration, Timer, Timestamp},
};
use std::collections::BTreeMap;

struct TestEndpoint {
    addr: SocketAddress,
    tx_message_id: u32,
    rx_messages: BTreeMap<u32, Timestamp>,
    total_messages: u32,
    subscriber: NoopSubscriber,
    close_timer: Timer,
}

impl TestEndpoint {
    fn new(addr: SocketAddress) -> Self {
        Self {
            addr,
            tx_message_id: 0,
            rx_messages: BTreeMap::new(),
            total_messages: 1000,
            subscriber: Default::default(),
            close_timer: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct NoopSubscriber;

impl event::Subscriber for NoopSubscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}

impl Endpoint for TestEndpoint {
    type PathHandle = PathHandle;
    type Subscriber = NoopSubscriber;

    const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;

    fn transmit<Tx: tx::Queue<Handle = PathHandle>, C: Clock>(
        &mut self,
        queue: &mut Tx,
        _clock: &C,
    ) {
        while self.tx_message_id < self.total_messages {
            let payload = self.tx_message_id.to_be_bytes();
            let addr = PathHandle::from_remote_address(self.addr.into());
            let msg = (addr, payload);
            if queue.push(msg).is_ok() {
                self.tx_message_id += 1;
            } else {
                // no more capacity
                return;
            }
        }
    }

    fn receive<Rx: rx::Queue<Handle = PathHandle>, C: Clock>(&mut self, queue: &mut Rx, clock: &C) {
        let now = clock.get_time();
        queue.for_each(|_header, payload| {
            assert_eq!(payload.len(), 4, "invalid payload {payload:?}");

            let id = (&*payload).try_into().unwrap();
            let id = u32::from_be_bytes(id);
            self.rx_messages.insert(id, now);
        });
    }

    fn poll_wakeups<C: Clock>(
        &mut self,
        _cx: &mut Context<'_>,
        clock: &C,
    ) -> Poll<Result<usize, CloseError>> {
        let now = clock.get_time();

        if self.close_timer.poll_expiration(now).is_ready() {
            assert!(self.rx_messages.len() as u32 * 4 > self.total_messages);
            return Err(CloseError).into();
        }

        if !self.close_timer.is_armed()
            && self.total_messages <= self.tx_message_id
            && !self.rx_messages.is_empty()
        {
            self.close_timer.set(now + Duration::from_millis(100));
        }

        Poll::Pending
    }

    fn timeout(&self) -> Option<Timestamp> {
        self.close_timer.next_expiration()
    }

    fn set_mtu_config(&mut self, _mtu_config: mtu::Config) {
        // noop
    }

    fn subscriber(&mut self) -> &mut Self::Subscriber {
        &mut self.subscriber
    }
}

fn bind(port: u16) -> std::net::SocketAddr {
    use std::net::Ipv4Addr;
    (Ipv4Addr::UNSPECIFIED, port).into()
}

#[test]
fn sim_test() -> io::Result<()> {
    use turmoil::lookup;

    let mut sim = turmoil::Builder::new().build();

    sim.client("client", async move {
        let io = Io::builder().with_address(bind(123))?.build()?;

        let endpoint = TestEndpoint::new((lookup("server"), 456).into());

        let (task, _) = io.start(endpoint)?;

        task.await?;

        Ok(())
    });

    sim.client("server", async move {
        let io = Io::builder().with_address(bind(456))?.build()?;

        let endpoint = TestEndpoint::new((lookup("client"), 123).into());

        let (task, _) = io.start(endpoint)?;

        task.await?;

        Ok(())
    });

    sim.run().unwrap();

    Ok(())
}

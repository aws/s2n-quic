// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{AddressMap, Client, Connection};
use crate::{
    driver::timer::{self, Timer, Timestamp},
    operation as op, scenario, Checkpoints, Result, Trace,
};
use core::{
    future::Future,
    task::{Context, Poll},
};
use futures::ready;

pub struct Thread<'a, C: Client<'a>> {
    scenario: &'a scenario::Client,
    ops: &'a [op::Client],
    index: usize,
    op: Option<Op<'a, C>>,
    timer: Timer,
}

impl<'a, C: Client<'a>> Thread<'a, C> {
    pub fn new(scenario: &'a scenario::Client, ops: &'a [op::Client]) -> Self {
        Self {
            scenario,
            ops,
            index: 0,
            op: None,
            timer: Timer::default(),
        }
    }

    pub(super) fn poll<T: Trace, Ch: Checkpoints>(
        &mut self,
        client: &mut C,
        address_map: &AddressMap,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        loop {
            while self.op.is_none() {
                if let Some(op) = self.ops.get(self.index) {
                    self.index += 1;
                    self.on_op(client, address_map, op, trace, checkpoints, now, cx);
                } else {
                    // we are all done processing the operations
                    return Poll::Ready(Ok(()));
                }
            }

            ready!(self.poll_op(client, address_map, trace, checkpoints, now, cx))?;
            self.op = None;
        }
    }

    fn on_op<T: Trace, Ch: Checkpoints>(
        &mut self,
        client: &mut C,
        addresses: &AddressMap,
        op: &'a op::Client,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: Timestamp,
        cx: &mut Context,
    ) {
        trace.exec_client(now, op);
        use op::Client::*;
        match op {
            Sleep { timeout } => {
                self.timer.sleep(now, *timeout);
                self.op = Some(Op::Sleep);
            }
            Connect {
                server_id,
                router_id,
                server_connection_id,
                client_connection_id,
            } => {
                let addr = if let Some(router_id) = router_id {
                    addresses.router(*router_id, *server_id)
                } else {
                    addresses.server(*server_id)
                };
                let hostname = addresses.hostname(*server_connection_id);
                let ops = &self.scenario.connections[*client_connection_id as usize];
                let connect = client.connect(addr, hostname, *server_connection_id, ops);
                self.op = Some(Op::Connect {
                    connect,
                    id: *client_connection_id,
                    start: now,
                });
            }
            Park { checkpoint } => {
                trace.park(now, *checkpoint);
                self.op = Some(Op::Wait {
                    checkpoint: *checkpoint,
                });
            }
            Unpark { checkpoint } => {
                checkpoints.unpark(*checkpoint, cx);
            }
            Trace { trace_id } => {
                trace.trace(now, *trace_id);
            }
            Scope { threads } => {
                if !threads.is_empty() {
                    let threads = threads
                        .iter()
                        .map(|thread| Thread::new(self.scenario, thread))
                        .collect();
                    self.op = Some(Op::Scope { threads });
                }
            }
        }
    }

    fn poll_op<T: Trace, Ch: Checkpoints>(
        &mut self,
        client: &mut C,
        addresses: &AddressMap,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        match self.op.as_mut().unwrap() {
            Op::Sleep => {
                ready!(self.timer.poll(now));
            }
            Op::Connect { connect, id, start } => {
                let connect = core::pin::Pin::new(connect);
                let connection = ready!(connect.poll(cx))?;
                let time = now - *start;
                trace.connect(now, *id, time);
                self.op = Some(Op::Connection { connection });
                return self.poll_op(client, addresses, trace, checkpoints, now, cx);
            }
            Op::Connection { connection } => {
                ready!(connection.poll(trace, checkpoints, now, cx))?;
            }
            Op::Wait { checkpoint } => {
                ready!(checkpoints.park(*checkpoint));
                trace.unpark(now, *checkpoint);
            }
            Op::Scope { threads } => {
                let mut all_ready = true;
                let op_idx = self.index;
                for (idx, thread) in threads.iter_mut().enumerate() {
                    trace.enter(now, op_idx as _, idx);
                    let result = thread.poll(client, addresses, trace, checkpoints, now, cx);
                    trace.exit(now);
                    match result {
                        Poll::Ready(Ok(_)) => {}
                        Poll::Ready(Err(err)) => return Err(err).into(),
                        Poll::Pending => all_ready = false,
                    }
                }
                if !all_ready {
                    return Poll::Pending;
                }
            }
        }

        // clear the timer for the next operation
        self.timer.cancel();
        Ok(()).into()
    }
}

impl<'a, C: Client<'a>> timer::Provider for Thread<'a, C> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.timer.timers(query)?;
        match &self.op {
            Some(Op::Connection { connection }) => {
                connection.timers(query)?;
            }
            Some(Op::Scope { threads }) => {
                for thread in threads {
                    thread.timers(query)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

enum Op<'a, C: Client<'a>> {
    Sleep,
    Connect {
        connect: C::Connect,
        id: u64,
        start: Timestamp,
    },
    Connection {
        connection: C::Connection,
    },
    Wait {
        checkpoint: u64,
    },
    Scope {
        threads: Vec<Thread<'a, C>>,
    },
}

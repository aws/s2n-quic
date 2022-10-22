// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::timer::{self, Timer, Timestamp};
use crate::{
    connection::Owner,
    operation as op,
    units::{Byte, Rate, Rates},
    Checkpoints, Connection, Result, Trace,
};
use core::task::{Context, Poll};
use futures::ready;

#[derive(Debug)]
pub struct Thread<'a> {
    ops: &'a [op::Connection],
    index: usize,
    op: Option<Op<'a>>,
    timer: Timer,
    owner: Owner,
}

impl<'a> Thread<'a> {
    #[inline]
    pub fn new(ops: &'a [op::Connection], owner: Owner) -> Self {
        Self {
            ops,
            index: 0,
            op: None,
            timer: Timer::default(),
            owner,
        }
    }

    #[inline]
    pub(crate) fn poll<C: Connection, T: Trace, Ch: Checkpoints>(
        &mut self,
        conn: &mut C,
        trace: &mut T,
        checkpoints: &mut Ch,
        rates: &mut Rates,
        now: Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        loop {
            while self.op.is_none() {
                if let Some(op) = self.ops.get(self.index) {
                    self.index += 1;
                    self.on_op(op, trace, checkpoints, rates, now, cx);
                } else {
                    // we are all done processing the operations
                    return Poll::Ready(Ok(()));
                }
            }

            ready!(self.poll_op(conn, trace, checkpoints, rates, now, cx))?;
            self.op = None;
        }
    }

    fn reset(&mut self, cx: &mut Context) {
        self.index = 0;
        self.op = None;
        cx.waker().wake_by_ref();
    }

    #[inline]
    fn on_op<T: Trace, Ch: Checkpoints>(
        &mut self,
        op: &'a op::Connection,
        trace: &mut T,
        checkpoints: &mut Ch,
        rates: &mut Rates,
        now: Timestamp,
        cx: &mut Context,
    ) {
        trace.exec(now, op);
        use op::{Connection::*, IterateValue};
        match op {
            Sleep { amount } => {
                self.timer.sleep(now, *amount);
                self.op = Some(Op::Sleep);
            }
            OpenBidirectionalStream { stream_id } => {
                self.op = Some(Op::OpenBidirectionalStream { id: *stream_id });
            }
            OpenSendStream { stream_id } => {
                self.op = Some(Op::OpenSendStream { id: *stream_id });
            }
            Send { stream_id, bytes } => {
                self.op = Some(Op::Send {
                    id: *stream_id,
                    remaining: *bytes,
                    rate: rates.send.get(stream_id).cloned(),
                });
            }
            SendFinish { stream_id } => {
                self.op = Some(Op::SendFinish { id: *stream_id });
            }
            SendRate { stream_id, rate } => {
                rates.send.insert(*stream_id, *rate);
            }
            Receive { stream_id, bytes } => {
                self.op = Some(Op::Receive {
                    id: *stream_id,
                    remaining: *bytes,
                    rate: rates.receive.get(stream_id).cloned(),
                });
            }
            ReceiveAll { stream_id } => {
                self.op = Some(Op::ReceiveAll {
                    id: *stream_id,
                    rate: rates.receive.get(stream_id).cloned(),
                });
            }
            ReceiveFinish { stream_id } => {
                self.op = Some(Op::ReceiveFinish { id: *stream_id });
            }
            ReceiveRate { stream_id, rate } => {
                rates.receive.insert(*stream_id, *rate);
            }
            Trace { trace_id } => {
                trace.trace(now, *trace_id);
            }
            Profile {
                trace_id,
                operations,
            } => {
                self.op = Some(Op::Profile {
                    start: now,
                    trace_id: *trace_id,
                    thread: Box::new(Thread::new(operations, self.owner)),
                });
            }
            Iterate {
                value,
                operations,
                trace_id,
            } => {
                let start = now;
                let trace_id = *trace_id;
                let thread = Box::new(Thread::new(operations, self.owner));

                self.op = Some(match value {
                    IterateValue::Count { amount } => Op::Iterate {
                        start,
                        count: *amount,
                        thread,
                        trace_id,
                    },
                    IterateValue::Time { amount } => {
                        self.timer.sleep(now, *amount);
                        Op::IterateFor {
                            start,
                            thread,
                            trace_id,
                        }
                    }
                });
            }
            Park { checkpoint } => {
                trace.park(now, *checkpoint);
                self.op = Some(Op::Wait {
                    checkpoint: *checkpoint,
                });
            }
            Unpark { checkpoint } => {
                // notify the checkpoint that it can make progress
                checkpoints.unpark(*checkpoint, cx);
            }
            Scope { threads } => {
                if !threads.is_empty() {
                    let threads = threads
                        .iter()
                        .map(|thread| Thread::new(thread, self.owner))
                        .collect();
                    self.op = Some(Op::Scope { threads });
                }
            }
        }
    }

    #[inline]
    fn poll_op<C: Connection, T: Trace, Ch: Checkpoints>(
        &mut self,
        conn: &mut C,
        trace: &mut T,
        checkpoints: &mut Ch,
        rates: &mut Rates,
        now: Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        let owner = self.owner;
        match self.op.as_mut().unwrap() {
            Op::Sleep => {
                ready!(self.timer.poll(now));
            }
            Op::OpenBidirectionalStream { id } => {
                ready!(conn.poll_open_bidirectional_stream(*id, cx))?;
                trace.open(now, *id);
            }
            Op::OpenSendStream { id } => {
                ready!(conn.poll_open_send_stream(*id, cx))?;
                trace.open(now, *id);
            }
            Op::Send {
                id,
                remaining,
                rate,
            } => {
                return self.timer.transfer(remaining, rate, now, cx, |bytes, cx| {
                    let amount = ready!(conn.poll_send(owner, *id, *bytes, cx))?;
                    trace.send(now, *id, amount);
                    Ok(amount).into()
                })
            }
            Op::SendFinish { id } => {
                ready!(conn.poll_send_finish(owner, *id, cx))?;
                trace.send_finish(now, *id);
            }
            Op::Receive {
                id,
                remaining,
                rate,
            } => {
                return self.timer.transfer(remaining, rate, now, cx, |bytes, cx| {
                    let amount = ready!(conn.poll_receive(owner, *id, *bytes, cx))?;
                    trace.receive(now, *id, amount);
                    Ok(amount).into()
                })
            }
            Op::ReceiveAll { id, rate } => {
                let mut remaining = Byte::MAX;
                return self
                    .timer
                    .transfer(&mut remaining, rate, now, cx, |bytes, cx| {
                        let amount = ready!(conn.poll_receive(owner, *id, *bytes, cx))?;
                        trace.receive(now, *id, amount);
                        Ok(amount).into()
                    });
            }
            Op::ReceiveFinish { id } => {
                ready!(conn.poll_receive_finish(owner, *id, cx))?;
                trace.receive_finish(now, *id);
            }
            Op::Wait { checkpoint } => {
                ready!(checkpoints.park(*checkpoint));
                trace.unpark(now, *checkpoint);
            }
            Op::Iterate {
                start,
                count,
                thread,
                trace_id,
            } => {
                ready!(thread.poll(conn, trace, checkpoints, rates, now, cx))?;

                if let Some(trace_id) = trace_id {
                    let time = now.saturating_duration_since(*start);
                    trace.profile(now, *trace_id, time);
                    *start = now;
                }

                if let Some(next) = count.checked_sub(1) {
                    if next > 0 {
                        *count = next;
                        thread.reset(cx);
                        return Poll::Pending;
                    }
                }
            }
            Op::IterateFor {
                start,
                thread,
                trace_id,
            } => {
                if let Poll::Ready(res) = thread.poll(conn, trace, checkpoints, rates, now, cx) {
                    res?;
                    thread.reset(cx);

                    if let Some(trace_id) = trace_id {
                        let time = now.saturating_duration_since(*start);
                        trace.profile(now, *trace_id, time);
                        *start = now;
                    }
                }

                ready!(self.timer.poll(now));
            }
            Op::Profile {
                trace_id,
                start,
                thread,
            } => {
                ready!(thread.poll(conn, trace, checkpoints, rates, now, cx))?;
                let time = now.saturating_duration_since(*start);
                trace.profile(now, *trace_id, time);
            }
            Op::Scope { threads } => {
                let mut all_ready = true;
                let op_idx = self.index;
                for (idx, thread) in threads.iter_mut().enumerate() {
                    trace.enter(now, op_idx as _, idx);
                    let result = thread.poll(conn, trace, checkpoints, rates, now, cx);
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

impl timer::Provider for Thread<'_> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.timer.timers(query)?;
        match &self.op {
            Some(Op::Iterate { thread, .. })
            | Some(Op::IterateFor { thread, .. })
            | Some(Op::Profile { thread, .. }) => thread.timers(query)?,
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

#[derive(Debug)]
enum Op<'a> {
    Sleep,
    OpenBidirectionalStream {
        id: u64,
    },
    OpenSendStream {
        id: u64,
    },
    Send {
        id: u64,
        remaining: Byte,
        rate: Option<Rate>,
    },
    SendFinish {
        id: u64,
    },
    Receive {
        id: u64,
        remaining: Byte,
        rate: Option<Rate>,
    },
    ReceiveAll {
        id: u64,
        rate: Option<Rate>,
    },
    ReceiveFinish {
        id: u64,
    },
    Wait {
        checkpoint: u64,
    },
    Iterate {
        start: Timestamp,
        count: u64,
        thread: Box<Thread<'a>>,
        trace_id: Option<u64>,
    },
    IterateFor {
        start: Timestamp,
        thread: Box<Thread<'a>>,
        trace_id: Option<u64>,
    },
    Profile {
        trace_id: u64,
        start: Timestamp,
        thread: Box<Thread<'a>>,
    },
    Scope {
        threads: Vec<Thread<'a>>,
    },
}

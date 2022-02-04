// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::timer::Timer;
use crate::{
    connection::Owner,
    operation::ConnectionOperation,
    units::{Byte, Rate, Rates},
    Checkpoints, Connection, Result, Trace,
};
use core::task::{Context, Poll};
use futures::ready;

#[derive(Debug)]
pub struct Thread<'a> {
    ops: &'a [ConnectionOperation],
    index: usize,
    op: Option<Op<'a>>,
    timer: Timer,
    owner: Owner,
}

impl<'a> Thread<'a> {
    pub fn new(ops: &'a [ConnectionOperation], owner: Owner) -> Self {
        Self {
            ops,
            index: 0,
            op: None,
            timer: Timer::default(),
            owner,
        }
    }

    pub(crate) fn poll<C: Connection, T: Trace, Ch: Checkpoints>(
        &mut self,
        conn: &mut C,
        trace: &mut T,
        checkpoints: &mut Ch,
        rates: &mut Rates,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        loop {
            while self.op.is_none() {
                if let Some(op) = self.ops.get(self.index) {
                    self.index += 1;
                    self.on_op(op, trace, checkpoints, rates, cx);
                } else {
                    // we are all done processing the operations
                    return Poll::Ready(Ok(()));
                }
            }

            ready!(self.poll_op(conn, trace, checkpoints, rates, cx))?;
            self.op = None;
        }
    }

    fn on_op<T: Trace, Ch: Checkpoints>(
        &mut self,
        op: &'a ConnectionOperation,
        trace: &mut T,
        checkpoints: &mut Ch,
        rates: &mut Rates,
        cx: &mut Context,
    ) {
        trace.exec(op);
        match op {
            ConnectionOperation::Sleep { amount } => {
                self.timer.sleep(*amount);
                self.op = Some(Op::Sleep);
            }
            ConnectionOperation::OpenBidirectionalStream { stream_id } => {
                self.op = Some(Op::OpenBidirectionalStream { id: *stream_id });
            }
            ConnectionOperation::OpenSendStream { stream_id } => {
                self.op = Some(Op::OpenSendStream { id: *stream_id });
            }
            ConnectionOperation::Send { stream_id, bytes } => {
                self.op = Some(Op::Send {
                    id: *stream_id,
                    remaining: *bytes,
                    rate: rates.send.get(stream_id).cloned(),
                });
            }
            ConnectionOperation::SendFinish { stream_id } => {
                self.op = Some(Op::SendFinish { id: *stream_id });
            }
            ConnectionOperation::SendRate { stream_id, rate } => {
                rates.send.insert(*stream_id, *rate);
            }
            ConnectionOperation::Receive { stream_id, bytes } => {
                self.op = Some(Op::Receive {
                    id: *stream_id,
                    remaining: *bytes,
                    rate: rates.receive.get(stream_id).cloned(),
                });
            }
            ConnectionOperation::ReceiveAll { stream_id } => {
                self.op = Some(Op::ReceiveAll {
                    id: *stream_id,
                    rate: rates.receive.get(stream_id).cloned(),
                });
            }
            ConnectionOperation::ReceiveFinish { stream_id } => {
                self.op = Some(Op::ReceiveFinish { id: *stream_id });
            }
            ConnectionOperation::ReceiveRate { stream_id, rate } => {
                rates.receive.insert(*stream_id, *rate);
            }
            ConnectionOperation::Trace { trace_id } => {
                trace.trace(*trace_id);
            }
            ConnectionOperation::Park { checkpoint } => {
                self.op = Some(Op::Wait {
                    checkpoint: *checkpoint,
                });
            }
            ConnectionOperation::Unpark { checkpoint } => {
                // notify the checkpoint that it can make progress
                checkpoints.unpark(*checkpoint);
                // re-poll the operations since we may be unblocking another task
                cx.waker().wake_by_ref();
            }
            ConnectionOperation::Scope { threads } => {
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

    fn poll_op<C: Connection, T: Trace, Ch: Checkpoints>(
        &mut self,
        conn: &mut C,
        trace: &mut T,
        checkpoints: &mut Ch,
        rates: &mut Rates,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        let owner = self.owner;
        match self.op.as_mut().unwrap() {
            Op::Sleep => {
                ready!(self.timer.poll(cx));
                Poll::Ready(Ok(()))
            }
            Op::OpenBidirectionalStream { id } => conn.poll_open_bidirectional_stream(*id, cx),
            Op::OpenSendStream { id } => conn.poll_open_send_stream(*id, cx),
            Op::Send {
                id,
                remaining,
                rate,
            } => self.timer.transfer(remaining, rate, cx, |bytes, cx| {
                let amount = ready!(conn.poll_send(owner, *id, *bytes, cx))?;
                trace.send(*id, amount);
                Ok(amount).into()
            }),
            Op::SendFinish { id } => conn.poll_send_finish(owner, *id, cx),
            Op::Receive {
                id,
                remaining,
                rate,
            } => self.timer.transfer(remaining, rate, cx, |bytes, cx| {
                let amount = ready!(conn.poll_receive(owner, *id, *bytes, cx))?;
                trace.receive(*id, amount);
                Ok(amount).into()
            }),
            Op::ReceiveAll { id, rate } => {
                let mut remaining = Byte::MAX;
                self.timer.transfer(&mut remaining, rate, cx, |bytes, cx| {
                    let amount = ready!(conn.poll_receive(owner, *id, *bytes, cx))?;
                    trace.receive(*id, amount);
                    Ok(amount).into()
                })
            }
            Op::ReceiveFinish { id } => conn.poll_receive_finish(owner, *id, cx),
            Op::Wait { checkpoint } => {
                ready!(checkpoints.park(*checkpoint));
                Ok(()).into()
            }
            Op::Scope { threads } => {
                let mut all_ready = true;
                let op_idx = self.index;
                for (idx, thread) in threads.iter_mut().enumerate() {
                    trace.enter(op_idx, idx);
                    let result = thread.poll(conn, trace, checkpoints, rates, cx);
                    trace.exit();
                    match result {
                        Poll::Ready(Ok(_)) => {}
                        Poll::Ready(Err(err)) => return Err(err).into(),
                        Poll::Pending => all_ready = false,
                    }
                }
                if all_ready {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
        }
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
    Scope {
        threads: Vec<Thread<'a>>,
    },
}

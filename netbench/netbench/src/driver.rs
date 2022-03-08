// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{connection::Owner, units::Rates, Checkpoints, Connection, Result, Timer, Trace};
use core::task::{Context, Poll};
use futures::ready;

mod thread;
pub(crate) mod timer;

use thread::Thread;

#[derive(Debug)]
pub struct Driver<'a, C: Connection> {
    pub connection: C,
    local_thread: Thread<'a>,
    local_rates: Rates,
    peer_streams: Vec<(Poll<()>, Thread<'a>)>,
    peer_rates: Rates,
    can_accept: bool,
    is_finished: bool,
}

impl<'a, C: Connection> Driver<'a, C> {
    pub fn new(scenario: &'a crate::scenario::Connection, connection: C) -> Self {
        Self {
            connection,
            local_thread: Thread::new(&scenario.ops, Owner::Local),
            local_rates: Default::default(),
            peer_streams: scenario
                .peer_streams
                .iter()
                .map(|ops| (Poll::Pending, Thread::new(ops, Owner::Remote)))
                .collect(),
            peer_rates: Default::default(),
            can_accept: true,
            is_finished: false,
        }
    }

    pub async fn run<T: Trace, Ch: Checkpoints, Ti: Timer>(
        mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        timer: &mut Ti,
    ) -> Result<C> {
        futures::future::poll_fn(|cx| self.poll_with_timer(trace, checkpoints, timer, cx)).await?;
        Ok(self.connection)
    }

    pub fn poll_with_timer<T: Trace, Ch: Checkpoints, Ti: Timer>(
        &mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        timer: &mut Ti,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        let now = timer.now();
        let res = self.poll(trace, checkpoints, now, cx);

        if let Some(target) = timer::Provider::next_expiration(&self) {
            // update the timer with the next expiration
            let _ = timer.poll(target, cx);
        };

        res
    }

    pub fn poll<T: Trace, Ch: Checkpoints>(
        &mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: timer::Timestamp,
        cx: &mut Context,
    ) -> Poll<Result<()>> {
        if self.is_finished {
            return self.connection.poll_finish(cx);
        }

        let mut poll_accept = false;
        let mut all_ready = true;

        trace.enter(now, 0, 0);
        let result = self.local_thread.poll(
            &mut self.connection,
            trace,
            checkpoints,
            &mut self.local_rates,
            now,
            cx,
        );
        trace.exit(now);

        match result {
            Poll::Ready(Ok(_)) => {}
            Poll::Ready(Err(err)) => return Err(err).into(),
            Poll::Pending => all_ready = false,
        }

        for (idx, (accepted, thread)) in self.peer_streams.iter_mut().enumerate() {
            // if we're still waiting to accept this stream move on
            if accepted.is_pending() {
                all_ready = false;
                poll_accept = self.can_accept;
                continue;
            }

            trace.enter(now, 1, idx);
            let result = thread.poll(
                &mut self.connection,
                trace,
                checkpoints,
                &mut self.peer_rates,
                now,
                cx,
            );
            trace.exit(now);

            match result {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(err)) => return Err(err).into(),
                Poll::Pending => all_ready = false,
            }
        }

        if poll_accept {
            match self.connection.poll_accept_stream(cx) {
                Poll::Ready(Ok(Some(id))) => {
                    trace.accept(now, id);
                    if let Some((accepted, _)) = self.peer_streams.get_mut(id as usize) {
                        *accepted = Poll::Ready(());
                        cx.waker().wake_by_ref();
                    } else {
                        todo!("return a not found error")
                    }
                }
                Poll::Ready(Ok(None)) => self.can_accept = false,
                Poll::Ready(Err(err)) => return Err(err).into(),
                Poll::Pending => all_ready = false,
            }
        }

        all_ready &= !timer::Provider::is_armed(&self);

        if all_ready {
            self.is_finished = true;
            self.connection.poll_finish(cx)
        } else {
            ready!(self.connection.poll_progress(cx))?;
            Poll::Pending
        }
    }
}

impl<'a, C: Connection> crate::client::Connection for Driver<'a, C> {
    fn poll<T, Ch>(
        &mut self,
        trace: &mut T,
        checkpoints: &mut Ch,
        now: timer::Timestamp,
        cx: &mut Context<'_>,
    ) -> Poll<Result<()>>
    where
        T: Trace,
        Ch: Checkpoints,
    {
        Self::poll(self, trace, checkpoints, now, cx)
    }
}

impl<'a, C: Connection> timer::Provider for Driver<'a, C> {
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.local_thread.timers(query)?;
        for (_, thread) in self.peer_streams.iter() {
            thread.timers(query)?;
        }
        Ok(())
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    message::Message,
    socket::{ring::Producer, stats, task::events},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::task::cooldown::Cooldown;

pub use events::RxEvents as Events;

pub trait Socket<T: Message> {
    type Error;

    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [T],
        events: &mut Events,
        stats: &stats::Sender,
    ) -> Result<(), Self::Error>;
}

pub struct Receiver<T: Message, S: Socket<T>> {
    ring: Producer<T>,
    /// Primary socket (high priority in priority mode, only socket in normal mode)
    rx: S,
    /// Optional low-priority socket for priority scheduling
    socket_low: Option<S>,
    ring_cooldown: Cooldown,
    io_cooldown: Cooldown,
    stats: stats::Sender,
    has_registered_drop_waker: bool,
}

impl<T, S> Receiver<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    /// Creates a new Receiver.
    ///
    /// If `socket_low` is provided, the receiver operates in priority mode:
    /// `rx` is the high-priority socket (drained first), `socket_low` is the low-priority socket.
    #[inline]
    pub fn new(
        ring: Producer<T>,
        rx: S,
        socket_low: Option<S>,
        cooldown: Cooldown,
        stats: stats::Sender,
    ) -> Self {
        Self {
            ring,
            rx,
            socket_low,
            ring_cooldown: cooldown.clone(),
            io_cooldown: cooldown,
            stats,
            has_registered_drop_waker: false,
        }
    }

    #[inline]
    fn poll_ring(&mut self, watermark: u32, cx: &mut Context) -> Poll<Result<(), ()>> {
        loop {
            let is_loop = self.ring_cooldown.state().is_loop();

            let count = if is_loop {
                self.ring.acquire(watermark)
            } else {
                match self.ring.poll_acquire(watermark, cx) {
                    Poll::Ready(count) => count,
                    Poll::Pending if !self.ring.is_open() => return Err(()).into(),
                    Poll::Pending => 0,
                }
            };

            // if the number of free slots increased since last time then yield
            if count > 0 {
                self.ring_cooldown.on_ready();
                return Ok(()).into();
            }

            if is_loop && self.ring_cooldown.on_pending_task(cx).is_sleep() {
                continue;
            }

            return Poll::Pending;
        }
    }
}

impl<T, S> Future for Receiver<T, S>
where
    T: Message + Unpin,
    S: Socket<T> + Unpin,
{
    type Output = Option<S::Error>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();

        if !this.has_registered_drop_waker {
            this.has_registered_drop_waker = true;
            this.ring.register_drop_waker(cx);
        }

        let mut events = Events::default();
        let mut pending_wake = false;

        macro_rules! poll_ring {
            () => {
                match this.poll_ring(u32::MAX, cx) {
                    Poll::Ready(Ok(_)) => {}
                    Poll::Ready(Err(_)) => return None.into(),
                    Poll::Pending => {
                        if pending_wake {
                            this.ring.wake();
                        }
                        return Poll::Pending;
                    }
                }
            };
        }

        macro_rules! drain_socket {
            ($socket:expr $(, $on_recv:expr)?) => {{
                let entries = this.ring.data();
                match $socket.recv(cx, entries, &mut events, &this.stats) {
                    Ok(_) => {
                        let count = events.take_count() as u32;
                        if count > 0 {
                            $( this.stats.on_recv_socket_packets($on_recv, count as usize); )?
                            this.ring.release_no_wake(count);
                            this.io_cooldown.on_ready();
                            pending_wake = true;
                        }
                        Ok(count)
                    }
                    Err(err) => Err(err),
                }
            }};
        }

        if this.socket_low.is_some() {
            // Priority mode: drain high-priority socket completely, then allow
            // the low-priority socket a single drain before looping back.
            loop {
                // Drain the high-priority socket until it is blocked.
                while !events.take_blocked() {
                    poll_ring!();

                    if let Err(err) = drain_socket!(&mut this.rx, true) {
                        return Some(err).into();
                    }
                }

                // High-priority socket is blocked. Give the low-priority socket one drain call.
                poll_ring!();

                if let Err(err) = drain_socket!(this.socket_low.as_mut().unwrap(), false) {
                    return Some(err).into();
                }

                // If the low-priority socket is also blocked, we're done.
                // Otherwise loop back to drain the high-priority socket again.
                if events.take_blocked() {
                    break;
                }
            }
        } else {
            while !events.take_blocked() {
                poll_ring!();

                if let Err(err) = drain_socket!(&mut this.rx) {
                    return Some(err).into();
                }
            }
        }

        this.io_cooldown.on_pending_task(cx);

        if pending_wake {
            this.ring.wake();
        }

        if !this.ring.is_open() {
            return Poll::Ready(None);
        }
        Poll::Pending
    }
}

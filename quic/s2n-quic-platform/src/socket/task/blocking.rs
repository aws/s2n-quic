// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    features::Gso,
    message::Message,
    socket::{
        ring::{Consumer, Producer},
        task::{rx, tx},
    },
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use parking::Unparker;
use s2n_quic_core::time;
use std::{net::UdpSocket, sync::Arc};

mod clock;
mod simple;
#[cfg(unix)]
mod unix;

pub use clock::Clock;

struct ThreadWaker(Unparker);

impl std::task::Wake for ThreadWaker {
    #[inline]
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    #[inline]
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

pub fn endpoint<E, F>(setup: E)
where
    E: FnOnce(Clock) -> F,
    F: Future<Output = ()>,
{
    use time::Clock as _;

    let clock = Clock::default();
    let mut future = setup(clock.clone());
    let mut future = unsafe { Pin::new_unchecked(&mut future) };

    let (parker, unparker) = parking::pair();
    let waker = ThreadWaker(unparker);
    let waker = Arc::new(waker).into();
    let mut cx = Context::from_waker(&waker);

    loop {
        match Future::poll(future.as_mut(), &mut cx) {
            Poll::Ready(_) => return,
            Poll::Pending => {
                let target = clock.timer.load();

                if target == 0 {
                    parker.park();
                    continue;
                }

                let now = unsafe { clock.get_time().as_duration().as_micros() as u64 };
                let diff = target.saturating_sub(now);
                if diff > 1000 {
                    let timeout = core::time::Duration::from_micros(diff);
                    parker.park_timeout(timeout);
                }
                clock.timer.on_wake();
            }
        }
    }
}

pub fn tx<S, M>(
    socket: S,
    ring: Consumer<M>,
    gso: Gso,
) -> Result<(), <UdpSocket as tx::Socket<M>>::Error>
where
    S: Into<UdpSocket>,
    M: Message + Unpin,
    UdpSocket: tx::Socket<M>,
{
    let socket = socket.into();
    socket.set_nonblocking(false).unwrap();

    let task = tx::Sender::new(ring, socket, gso);

    if let Some(err) = poll_blocking(task) {
        Err(err)
    } else {
        Ok(())
    }
}

pub fn rx<S, M>(socket: S, ring: Producer<M>) -> Result<(), <UdpSocket as rx::Socket<M>>::Error>
where
    S: Into<UdpSocket>,
    M: Message + Unpin,
    UdpSocket: rx::Socket<M>,
{
    let socket = socket.into();
    socket.set_nonblocking(false).unwrap();

    let task = rx::Receiver::new(ring, socket);

    if let Some(err) = poll_blocking(task) {
        Err(err)
    } else {
        Ok(())
    }
}

#[inline]
fn poll_blocking<F: Future>(mut task: F) -> F::Output {
    // TODO use the pin! macro once stable
    let mut task = unsafe { Pin::new_unchecked(&mut task) };

    let (parker, unparker) = parking::pair();
    let waker = ThreadWaker(unparker);
    let waker = Arc::new(waker).into();
    let mut cx = Context::from_waker(&waker);

    let mut stalls = 0;

    loop {
        match task.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                stalls += 1;
                if stalls > 10 {
                    stalls = 0;
                    parker.park();
                }
                continue;
            }
        }
    }
}

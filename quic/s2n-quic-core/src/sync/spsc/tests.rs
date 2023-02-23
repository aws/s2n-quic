// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::{check, generator::*};
use core::task::{Context, Poll, Waker};
use futures_test::task::{new_count_waker, AwokenCount};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, TypeGenerator)]
enum Operation {
    Push(u16),
    AsyncPush(u16),
    Pop(u16),
    AsyncPop(u16),
    Clear,
    DropSend,
    DropRecv,
}

struct WakeState {
    waker: Waker,
    wake_count: AwokenCount,
    snapshot: Option<usize>,
}

impl Default for WakeState {
    fn default() -> Self {
        let (waker, wake_count) = new_count_waker();
        Self {
            waker,
            wake_count,
            snapshot: None,
        }
    }
}

impl WakeState {
    fn context(&self) -> Context {
        Context::from_waker(&self.waker)
    }

    fn snapshot(&mut self) {
        self.snapshot = Some(self.wake_count.get());
    }

    fn assert_wake(&mut self) {
        if let Some(prev) = self.snapshot.take() {
            assert_eq!(self.wake_count.get(), prev + 1);
        }
    }
}

struct Model<T> {
    oracle: VecDeque<T>,
    send: Option<Sender<T>>,
    send_waker: WakeState,
    recv: Option<Receiver<T>>,
    recv_waker: WakeState,
    capacity: usize,
}

impl<T: Clone + PartialEq + core::fmt::Debug> Model<T> {
    fn new(capacity: usize) -> Self {
        let (send, recv) = channel(capacity);
        let capacity = send.capacity();
        Self {
            oracle: Default::default(),
            send: Some(send),
            send_waker: Default::default(),
            recv: Some(recv),
            recv_waker: Default::default(),
            capacity,
        }
    }

    fn apply_all(&mut self, operations: &[Operation], mut generator: impl FnMut() -> T) {
        for op in operations {
            self.apply(*op, &mut generator);
        }
    }

    fn apply(&mut self, operation: Operation, mut generator: impl FnMut() -> T) {
        match operation {
            Operation::Push(count) => {
                let mut did_push = false;
                if let Some(send) = self.send.as_mut() {
                    match send.try_slice() {
                        Ok(Some(mut slice)) => {
                            for _ in 0..count {
                                let value = generator();
                                if slice.push(value.clone()).is_ok() {
                                    self.oracle.push_back(value);
                                    did_push = true;
                                }
                            }
                        }
                        Ok(None) => {
                            assert_eq!(
                                self.oracle.len(),
                                self.capacity,
                                "slice should return None when at capacity"
                            );
                        }
                        Err(_) => {
                            assert!(self.recv.is_none());
                        }
                    }
                }

                if did_push {
                    self.recv_waker.assert_wake();
                }
            }
            Operation::AsyncPush(count) => {
                let mut did_push = false;
                if let Some(send) = self.send.as_mut() {
                    match send.poll_slice(&mut self.send_waker.context()) {
                        Poll::Ready(Ok(mut slice)) => {
                            for _ in 0..count {
                                let value = generator();
                                if slice.push(value.clone()).is_ok() {
                                    self.oracle.push_back(value);

                                    did_push = true;
                                }
                            }
                        }
                        Poll::Ready(Err(_)) => {
                            assert!(self.recv.is_none());
                        }
                        Poll::Pending => {
                            assert_eq!(
                                self.oracle.len(),
                                self.capacity,
                                "slice should return Pending when at capacity"
                            );
                            self.send_waker.snapshot();
                        }
                    }
                }

                if did_push {
                    self.recv_waker.assert_wake();
                }
            }
            Operation::Pop(count) => {
                let mut did_pop = false;
                if let Some(recv) = self.recv.as_mut() {
                    match recv.try_slice() {
                        Ok(Some(mut slice)) => {
                            for _ in 0..count {
                                let value = slice.pop();
                                assert_eq!(value, self.oracle.pop_front());
                                did_pop |= value.is_some();
                            }
                        }
                        Ok(None) => {
                            assert!(self.oracle.is_empty());
                        }
                        Err(_) => {
                            assert!(self.send.is_none());
                            assert!(self.oracle.is_empty());
                        }
                    }
                }

                if did_pop {
                    self.send_waker.assert_wake();
                }
            }
            Operation::AsyncPop(count) => {
                let mut did_pop = false;
                if let Some(recv) = self.recv.as_mut() {
                    match recv.poll_slice(&mut self.recv_waker.context()) {
                        Poll::Ready(Ok(mut slice)) => {
                            for _ in 0..count {
                                let value = slice.pop();
                                assert_eq!(value, self.oracle.pop_front());
                                did_pop |= value.is_some();
                            }
                        }
                        Poll::Ready(Err(_)) => {
                            assert!(self.send.is_none());
                            assert!(self.oracle.is_empty());
                        }
                        Poll::Pending => {
                            assert!(self.oracle.is_empty());
                            self.recv_waker.snapshot();
                        }
                    }
                }

                if did_pop {
                    self.send_waker.assert_wake();
                }
            }
            Operation::Clear => {
                let mut did_pop = false;
                if let Some(recv) = self.recv.as_mut() {
                    match recv.try_slice() {
                        Ok(Some(mut slice)) => {
                            // we need to pull in the latest values to clear everything
                            while slice.clear() > 0 {
                                did_pop = true;
                                let _ = slice.peek();
                            }
                            self.oracle.clear();
                        }
                        Ok(None) => {
                            assert!(self.oracle.is_empty());
                        }
                        Err(_) => {
                            assert!(self.send.is_none());
                            assert!(self.oracle.is_empty());
                        }
                    }
                }

                if did_pop {
                    self.send_waker.assert_wake();
                }
            }
            Operation::DropSend => {
                self.send = None;
            }
            Operation::DropRecv => {
                self.recv = None;
            }
        }
    }

    fn finish(mut self) {
        loop {
            self.apply(Operation::Pop(u16::MAX), || unimplemented!());
            if self.oracle.is_empty() || self.recv.is_none() {
                return;
            }
        }
    }
}

#[cfg(any(kani, miri))]
type Operations = crate::testing::InlineVec<Operation, 2>;
#[cfg(not(any(kani, miri)))]
type Operations = Vec<Operation>;

#[test]
fn model() {
    let max_capacity = if cfg!(any(kani, miri)) { 2 } else { 64 };

    let generator = (1usize..max_capacity, gen::<Operations>());

    check!()
        .with_generator(generator)
        .for_each(|(capacity, ops)| {
            let mut model = Model::new(*capacity);
            let mut cursor = 0;
            let generator = || {
                let v = cursor;
                cursor += 1;
                Box::new(v)
            };

            model.apply_all(ops, generator);
            model.finish();
        })
}

#[test]
fn model_zst() {
    let max_capacity = if cfg!(any(kani, miri)) { 2 } else { 64 };

    let generator = (1usize..max_capacity, gen::<Operations>());

    check!()
        .with_generator(generator)
        .for_each(|(capacity, ops)| {
            let mut model = Model::new(*capacity);
            let generator = || ();

            model.apply_all(ops, generator);
            model.finish();
        })
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(3), kani::solver(kissat))]
fn alloc_test() {
    let capacity = if cfg!(any(kani, miri)) {
        1usize..3
    } else {
        1usize..4096
    };

    check!()
        .with_generator((capacity, gen::<u8>()))
        .cloned()
        .for_each(|(capacity, push_value)| {
            let (mut send, mut recv) = channel(capacity);

            send.try_slice().unwrap().unwrap().push(push_value).unwrap();

            let pop_value = recv.try_slice().unwrap().unwrap().pop().unwrap();
            assert_eq!(pop_value, push_value);
        })
}

#[cfg(not(loom))]
mod loom {
    pub use std::*;

    pub mod future {
        pub use futures::executor::block_on;
    }

    pub fn model<F: FnOnce() -> R, R>(f: F) -> R {
        f()
    }
}

const CAPACITY: usize = if cfg!(loom) { 2 } else { 10 };
const BATCH_COUNT: usize = if cfg!(loom) { 2 } else { 100 };
const BATCH_SIZE: usize = if cfg!(loom) { 3 } else { 20 };
const EXPECTED_COUNT: usize = BATCH_COUNT * BATCH_SIZE;

// TODO The async rx loom tests seem to take an unbounded amount if time if the batch count/size is
// anything bigger than 1. Ideally, these sizes would be bigger to test more permutations of
// orderings so we should investigate what's causing loom to endless spin.
const ASYNC_RX_BATCH_COUNT: usize = if cfg!(loom) { 1 } else { BATCH_COUNT };
const ASYNC_RX_BATCH_SIZE: usize = if cfg!(loom) { 1 } else { BATCH_SIZE };
const ASYNC_RX_EXPECTED_COUNT: usize = ASYNC_RX_BATCH_COUNT * ASYNC_RX_BATCH_SIZE;

#[test]
#[cfg_attr(miri, ignore)] // TODO https://github.com/aws/s2n-quic/issues/1635
fn loom_spin_tx_spin_rx_test() {
    loom_scenario(
        CAPACITY,
        |send| loom_spin_tx(send, BATCH_COUNT, BATCH_SIZE),
        |recv| loom_spin_rx(recv, EXPECTED_COUNT),
    )
}

#[test]
#[cfg_attr(miri, ignore)] // TODO https://github.com/aws/s2n-quic/issues/1635
fn loom_spin_tx_async_rx_test() {
    loom_scenario(
        CAPACITY,
        |send| loom_spin_tx(send, ASYNC_RX_BATCH_COUNT, ASYNC_RX_BATCH_SIZE),
        |recv| loom_async_rx(recv, ASYNC_RX_EXPECTED_COUNT),
    )
}

#[test]
#[cfg_attr(miri, ignore)] // TODO https://github.com/aws/s2n-quic/issues/1635
fn loom_async_tx_spin_rx_test() {
    loom_scenario(
        CAPACITY,
        |send| loom_async_tx(send, BATCH_COUNT, BATCH_SIZE),
        |recv| loom_spin_rx(recv, EXPECTED_COUNT),
    )
}

#[test]
#[cfg_attr(miri, ignore)] // TODO https://github.com/aws/s2n-quic/issues/1635
fn loom_async_tx_async_rx_test() {
    loom_scenario(
        CAPACITY,
        |send| loom_async_tx(send, ASYNC_RX_BATCH_COUNT, ASYNC_RX_BATCH_SIZE),
        |recv| loom_async_rx(recv, ASYNC_RX_EXPECTED_COUNT),
    )
}

fn loom_scenario(capacity: usize, sender: fn(Sender<u32>), receiver: fn(Receiver<u32>)) {
    loom::model(move || {
        let (send, recv) = channel(capacity);

        let a = loom::thread::spawn(move || sender(send));

        let b = loom::thread::spawn(move || receiver(recv));

        // loom tests will still run after returning so we don't need to join
        if cfg!(not(loom)) {
            a.join().unwrap();
            b.join().unwrap();
        }
    });
}

fn loom_spin_rx(mut recv: Receiver<u32>, expected: usize) {
    use loom::hint;

    let mut value = 0u32;
    loop {
        match recv.try_slice() {
            Ok(Some(mut slice)) => {
                while let Some(actual) = slice.pop() {
                    assert_eq!(actual, value);
                    value += 1;
                }
            }
            Ok(None) => hint::spin_loop(),
            Err(_) => {
                assert_eq!(value as usize, expected);
                return;
            }
        }
    }
}

fn loom_async_rx(mut recv: Receiver<u32>, expected: usize) {
    use futures::{future::poll_fn, ready};

    loom::future::block_on(async move {
        let mut value = 0u32;
        poll_fn(|cx| loop {
            match ready!(recv.poll_slice(cx)) {
                Ok(mut slice) => {
                    while let Some(actual) = slice.pop() {
                        assert_eq!(actual, value);
                        value += 1;
                    }
                }
                Err(_err) => return Poll::Ready(()),
            }
        })
        .await;

        assert_eq!(value as usize, expected);
    });
}

fn loom_spin_tx(mut send: Sender<u32>, batch_count: usize, batch_size: usize) {
    use loom::hint;

    let max_value = (batch_count * batch_size) as u32;
    let mut value = 0u32;

    'done: while max_value > value {
        let mut remaining = batch_size;
        while remaining > 0 {
            match send.try_slice() {
                Ok(Some(mut slice)) => {
                    let num_items = remaining;
                    for _ in 0..num_items {
                        if slice.push(value).is_err() {
                            hint::spin_loop();
                            continue;
                        }
                        value += 1;
                        remaining -= 1;
                    }
                }
                Ok(None) => {
                    // we don't have capacity to send so yield the thread
                    hint::spin_loop();
                }
                Err(_) => {
                    // The peer dropped the channel so bail
                    break 'done;
                }
            }
        }
    }

    assert_eq!(value, max_value);
}

fn loom_async_tx(mut send: Sender<u32>, batch_count: usize, batch_size: usize) {
    use futures::{future::poll_fn, ready};

    loom::future::block_on(async move {
        let max_value = (batch_count * batch_size) as u32;
        let mut value = 0u32;

        while max_value > value {
            let mut remaining = batch_size;
            let result = poll_fn(|cx| loop {
                return match ready!(send.poll_slice(cx)) {
                    Ok(mut slice) => {
                        let num_items = remaining;
                        for _ in 0..num_items {
                            if slice.push(value).is_err() {
                                // try polling the slice capacity again
                                break;
                            }
                            value += 1;
                            remaining -= 1;
                        }

                        if remaining > 0 {
                            continue;
                        }

                        Ok(())
                    }
                    Err(err) => Err(err),
                }
                .into();
            })
            .await;

            if result.is_err() {
                return;
            }
        }

        assert_eq!(value, max_value);
    });
}

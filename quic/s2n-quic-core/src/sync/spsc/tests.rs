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
                            self.oracle.clear();
                            did_pop = slice.clear() > 0;
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

type Operations = Vec<Operation>;

#[test]
fn model() {
    check!()
        .with_generator((1usize..64, gen::<Operations>()))
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

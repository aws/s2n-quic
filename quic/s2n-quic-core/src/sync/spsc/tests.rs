use super::generic::*;
use bolero::{check, generator::*};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, TypeGenerator)]
enum Operation {
    Push(u16),
    Pop(u16),
    DropSend,
    DropRecv,
}

struct Model<T, B: Behavior> {
    oracle: VecDeque<T>,
    send: Option<Sender<T, B>>,
    recv: Option<Receiver<T, B>>,
    capacity: usize,
}

impl<T: Clone + PartialEq + core::fmt::Debug, B: Behavior> Model<T, B> {
    fn new(capacity: usize) -> Self {
        let (send, recv) = channel(capacity);
        Self {
            oracle: Default::default(),
            send: Some(send),
            recv: Some(recv),
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
                if let Some(send) = self.send.as_mut() {
                    match send.try_slice() {
                        Ok(Some(mut slice)) => {
                            for _ in 0..count {
                                let value = generator();
                                if slice.push(value.clone()).is_ok() {
                                    self.oracle.push_back(value);
                                }
                            }
                        }
                        Ok(None) => {
                            assert_eq!(self.oracle.len(), self.capacity);
                        }
                        Err(_) => {
                            assert!(self.recv.is_none());
                        }
                    }
                }
            }
            Operation::Pop(count) => {
                if let Some(recv) = self.recv.as_mut() {
                    match recv.try_slice() {
                        Ok(Some(mut slice)) => {
                            for _ in 0..count {
                                assert_eq!(slice.pop(), self.oracle.pop_front());
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

fn generic_model<B: Behavior>(capacity: usize, ops: &[Operation]) {
    let mut model = Model::<_, B>::new(capacity);
    let mut cursor = 0;
    let generator = || {
        let v = cursor;
        cursor += 1;
        Box::new(v)
    };

    model.apply_all(ops, generator);
    model.finish();
}

#[test]
fn ring() {
    check!()
        .with_generator((1usize..64, gen::<Operations>()))
        .for_each(|(capacity, ops)| generic_model::<Ring>(*capacity, ops))
}

#[test]
fn double_ring() {
    check!()
        .with_generator((1usize..64, gen::<Operations>()))
        .for_each(|(capacity, ops)| generic_model::<DoubleRing>(*capacity, ops))
}

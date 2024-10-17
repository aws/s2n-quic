// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::{check, TypeGenerator};
use core::fmt;

struct Model<T> {
    subject: RingDeque<T>,
    oracle: VecDeque<T>,
    open: bool,
}

impl<T> Default for Model<T> {
    fn default() -> Self {
        Self::new(32)
    }
}

impl<T> Model<T> {
    pub fn new(cap: usize) -> Self {
        Self {
            subject: RingDeque::new(cap),
            oracle: VecDeque::with_capacity(cap),
            open: true,
        }
    }
}

impl<T: fmt::Debug + Clone + PartialEq> Model<T> {
    pub fn pop_front(&mut self) -> Result<Option<T>, Closed> {
        let expected = if self.open {
            Ok(self.oracle.pop_front())
        } else {
            Err(Closed)
        };
        let actual = self.subject.pop_front();
        assert_eq!(expected, actual);
        actual
    }

    pub fn pop_back(&mut self) -> Result<Option<T>, Closed> {
        let expected = if self.open {
            Ok(self.oracle.pop_back())
        } else {
            Err(Closed)
        };
        let actual = self.subject.pop_back();
        assert_eq!(expected, actual);
        actual
    }

    pub fn push_front(&mut self, v: T) -> Result<Option<T>, Closed> {
        let actual = self.subject.push_front(v.clone());
        let expected = if self.open {
            let prev = if self.oracle.capacity() == self.oracle.len() {
                self.oracle.pop_back()
            } else {
                None
            };
            self.oracle.push_front(v);
            Ok(prev)
        } else {
            Err(Closed)
        };
        assert_eq!(expected, actual);
        actual
    }

    pub fn push_back(&mut self, v: T) -> Result<Option<T>, Closed> {
        let actual = self.subject.push_back(v.clone());
        let expected = if self.open {
            let prev = if self.oracle.capacity() == self.oracle.len() {
                self.oracle.pop_front()
            } else {
                None
            };
            self.oracle.push_back(v);
            Ok(prev)
        } else {
            Err(Closed)
        };
        assert_eq!(expected, actual);
        actual
    }

    pub fn close(&mut self) -> Result<(), Closed> {
        let actual = self.subject.close();
        let expected = if self.open {
            self.open = false;
            Ok(())
        } else {
            Err(Closed)
        };
        assert_eq!(actual, expected);
        actual
    }
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
enum Operation {
    PushFront,
    PushBack,
    PopFront,
    PopBack,
    Close,
}

#[test]
fn model_test() {
    check!().with_type::<Vec<Operation>>().for_each(|ops| {
        let mut v = 0;
        let mut model = Model::<u64>::default();
        for op in ops {
            match op {
                Operation::PushFront => {
                    let _ = model.push_front(v);
                    v += 1;
                }
                Operation::PushBack => {
                    let _ = model.push_back(v);
                    v += 1;
                }
                Operation::PopFront => {
                    let _ = model.pop_front();
                }
                Operation::PopBack => {
                    let _ = model.pop_back();
                }
                Operation::Close => {
                    let _ = model.close();
                }
            }
        }
    })
}

#[test]
fn overflow_front_test() {
    let mut model = Model::new(4);
    let _ = model.push_front(0);
    let _ = model.push_front(1);
    let _ = model.push_front(2);
    let _ = model.push_front(3);
    assert_eq!(model.push_front(4), Ok(Some(0)));

    assert_eq!(model.oracle.make_contiguous(), &[4, 3, 2, 1]);
}

#[test]
fn overflow_back_test() {
    let mut model = Model::new(4);
    let _ = model.push_back(0);
    let _ = model.push_back(1);
    let _ = model.push_back(2);
    let _ = model.push_back(3);
    assert_eq!(model.push_back(4), Ok(Some(0)));

    assert_eq!(model.oracle.make_contiguous(), &[1, 2, 3, 4]);
}

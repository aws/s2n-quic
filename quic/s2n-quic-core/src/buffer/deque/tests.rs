// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::buffer::writer::Storage as _;
use bolero::{check, TypeGenerator};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, TypeGenerator)]
enum Op {
    Recv { amount: u16, skip: u8 },
    Consume { amount: u16 },
    MakeContiguous,
    Clear,
}

#[derive(Debug)]
struct Model {
    oracle: VecDeque<u8>,
    subject: Deque,
    remaining_capacity: usize,
    byte: u8,
}

impl Default for Model {
    fn default() -> Self {
        let buffer = Deque::new(u16::MAX as _);
        let remaining_capacity = buffer.capacity();
        Self {
            oracle: Default::default(),
            subject: buffer,
            remaining_capacity,
            byte: 0,
        }
    }
}

impl Model {
    fn apply_all(&mut self, ops: &[Op]) {
        for op in ops {
            self.apply(*op);
        }
    }

    #[inline]
    fn pattern(&mut self, amount: usize, skip: u8) -> impl Iterator<Item = u8> + Clone {
        let base = self.byte as usize + skip as usize;

        let iter = (0u8..u8::MAX).cycle().skip(base).take(amount);

        self.byte = (base + amount) as u8;

        iter
    }

    fn apply(&mut self, op: Op) {
        match op {
            Op::Recv { amount, skip } => {
                let amount = self.remaining_capacity.min(amount as usize);
                let mut pattern = self.pattern(amount, skip);

                self.oracle.extend(pattern.clone());

                let mut pair = self.subject.unfilled();

                if amount > 0 {
                    assert!(!pair.is_empty());
                }

                assert!(amount <= pair.remaining_capacity());

                // copy the pattern into the unfilled portion
                for (a, b) in pair.iter_mut().flat_map(|s| s.iter_mut()).zip(&mut pattern) {
                    *a = MaybeUninit::new(b);
                }

                assert!(pattern.next().is_none(), "pattern should be drained");

                unsafe {
                    self.subject.fill(amount).unwrap();
                }

                self.remaining_capacity -= amount;
            }
            Op::Consume { amount } => {
                let amount = self.oracle.len().min(amount as usize);
                self.subject.consume(amount as _);

                if amount > 0 {
                    self.oracle.drain(..amount);
                    self.remaining_capacity += amount;
                }
            }
            Op::MakeContiguous => {
                self.subject.make_contiguous();
            }
            Op::Clear => {
                self.subject.clear();
                self.oracle.clear();
                self.remaining_capacity = self.subject.capacity();
            }
        }

        self.invariants();
    }

    fn invariants(&mut self) {
        assert_eq!(self.subject.len(), self.oracle.len());

        let subject = {
            let (head, tail) = self.subject.filled().into();
            head.iter().chain(&*tail)
        };
        let oracle = {
            let (head, tail) = self.oracle.as_slices();
            head.iter().chain(tail)
        };

        assert!(
            subject.eq(oracle),
            "subject ({:?}) == oracle ({:?})",
            {
                let (head, tail) = self.subject.filled().into();
                (&head[..head.len().min(10)], &tail[..tail.len().min(10)])
            },
            {
                let (head, tail) = self.oracle.as_slices();
                (&head[..head.len().min(10)], &tail[..tail.len().min(10)])
            }
        );

        assert_eq!(self.subject.remaining_capacity(), self.remaining_capacity);
    }
}

#[test]
fn model_test() {
    check!().with_type::<Vec<Op>>().for_each(|ops| {
        let mut model = Model::default();
        model.apply_all(ops);
    })
}

#[test]
fn from_vec_test() {
    check!().for_each(|slice| {
        let extra_capacity = slice.get(..2).map_or(0, |cap| {
            let cap: &[u8; 2] = cap.try_into().unwrap();
            u16::from_ne_bytes(*cap) as usize
        });

        let mut vec = slice.to_vec();
        vec.reserve_exact(extra_capacity);
        let mut buffer: Deque = vec.into();

        let (head, _tail) = buffer.filled().into();
        assert_eq!(slice, head);
    })
}

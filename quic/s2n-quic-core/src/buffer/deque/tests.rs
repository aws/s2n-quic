// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::buffer::writer::Storage as _;
use bolero::{check, TypeGenerator};
use std::collections::VecDeque;

// shrink the search space with kani
const CAPACITY: usize = if cfg!(kani) { 4 } else { u8::MAX as usize + 1 };
const OPS_LEN: usize = if cfg!(kani) { 2 } else { u8::MAX as usize + 1 };

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
        let capacity = CAPACITY;

        let buffer = Deque::new(capacity);
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
        self.invariants();
    }

    #[inline]
    fn pattern(&mut self, amount: usize, skip: u8) -> impl Iterator<Item = u8> + Clone {
        let base = self.byte as usize + skip as usize;

        let iter = core::iter::repeat(base as u8).take(amount);

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
                    assert!(pair.has_remaining_capacity());
                }

                assert!(amount <= pair.remaining_capacity());

                // copy the pattern into the unfilled portion
                for (a, b) in pair.iter_mut().zip(&mut pattern) {
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
    }

    #[cfg(not(kani))]
    fn invariants(&mut self) {
        self.invariants_common();

        let filled = self.subject.filled();
        let subject = filled.iter();
        let oracle = self.oracle.iter();

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

        // we include the length just to make sure the case where we exceed the length returns
        // `None`
        for idx in 0..=self.subject.len() {
            assert_eq!(self.oracle.get(idx), self.subject.filled().get(idx));
        }
    }

    #[cfg(kani)]
    fn invariants(&mut self) {
        self.invariants_common();

        let idx = kani::any();
        // we include the length just to make sure the case where we exceed the length returns
        // `None`
        kani::assume(idx <= self.subject.len());

        assert_eq!(self.oracle.get(idx), self.subject.filled().get(idx));
    }

    fn invariants_common(&self) {
        assert_eq!(self.subject.len(), self.oracle.len());
        assert_eq!(self.subject.remaining_capacity(), self.remaining_capacity);
    }
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(cadical))]
// even with the minimal amount of parameter bounds, this proof's memory consumption explodes
#[cfg_attr(kani, cfg(kani_slow))]
fn model_test() {
    let ops = bolero::gen::<Vec<Op>>().with().len(..=OPS_LEN);

    check!().with_generator(ops).for_each(|ops| {
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

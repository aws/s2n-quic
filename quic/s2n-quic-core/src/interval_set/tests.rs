// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::interval_set::IntervalSet;
use alloc::collections::BTreeSet;
use bolero::{check, generator::*};
use core::{
    num::NonZeroUsize,
    ops::{Range, RangeInclusive},
};

type RangeBound = u8;

#[derive(Clone, Debug, TypeGenerator)]
pub enum RangeValue {
    Range(Range<RangeBound>),
    RangeInclusive(RangeInclusive<RangeBound>),
}

#[derive(Clone, Debug, TypeGenerator)]
pub enum Operation {
    Insert { range: RangeValue },
    Remove { range: RangeValue },
}

#[derive(Clone, Debug, TypeGenerator)]
pub struct OperationTest {
    operations: Vec<Operation>,
    limit: Option<usize>,
}

macro_rules! assert_set_eq {
    ($expected:ident, $actual:ident) => {{
        // put all of the contained values into a vec for easy comparison
        let actual_items = $actual.iter().collect::<Vec<_>>();

        // make sure the contained values are exactly the same
        assert_eq!(
            $expected.iter().collect::<Vec<_>>(),
            actual_items,
            "values don't match\n expected:\t{:?}\n actual:\t{:?}",
            $expected,
            $actual
        );

        // test that the reverse iterator works as well
        let mut reversed_items = $actual.iter().rev().collect::<Vec<_>>();
        reversed_items.reverse();
        assert_eq!(
            actual_items, reversed_items,
            "reverse iterator isn't valid {:?}",
            reversed_items
        );

        // make sure the item counts are correct
        assert_eq!(
            $expected.count(),
            $actual.count(),
            "element lengths don't match\n expected:\t{:?}\n actual:\t{:?}",
            $expected,
            $actual
        );

        // make sure the number of intervals is the same
        assert_eq!(
            $expected.interval_len(),
            $actual.interval_len(),
            "interval lengths don't match\n expected:\t{:?}\n actual:\t{:?}",
            $expected,
            $actual
        );
    }};
}

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn interval_set_test() {
    check!().with_type().for_each(
        |(initial_ops, union_ops, difference_ops, intersection_ops)| {
            // creates an initial set given a Vec of insertions and deletions
            let (oracle, subject) = process_operation(initial_ops);

            /// performs a specific set operation to both implementations
            /// and asserts equivalence
            macro_rules! set_op_test {
                ($name:ident, $ops:ident) => {{
                    let (mut expected, mut actual) = process_operation($ops);
                    expected.$name(&oracle);
                    actual.remove_limit();
                    actual.$name(&subject).expect("invalid op");
                    assert_set_eq!(expected, actual);
                }};
            }

            set_op_test!(union, union_ops);
            set_op_test!(difference, difference_ops);
            set_op_test!(intersection, intersection_ops);
        },
    );
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(2))]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn interval_set_inset_range_test() {
    // Generate valid ranges (lb <= ub)
    let gen = gen::<(i32, i32, i32)>().filter_gen(|(a, b, _c)| a <= b);

    check!().with_generator(gen).for_each(|(lb, ub, elem)| {
        let mut set: IntervalSet<i32> = IntervalSet::new();
        let range = lb..=ub;
        assert!(set.insert(range.clone()).is_ok());
        assert_eq!(range.contains(&elem), set.contains(elem));
    });
}

fn process_operation(
    OperationTest { limit, operations }: &OperationTest,
) -> (Oracle, IntervalSet<RangeBound>) {
    let limit = limit.and_then(NonZeroUsize::new);
    let mut oracle = Oracle::new();
    let mut subject = if let Some(limit) = limit {
        IntervalSet::with_limit(limit)
    } else {
        IntervalSet::new()
    };

    macro_rules! perform_operation {
        ($operation:ident, $range:expr) => {{
            match $range {
                RangeValue::Range(range) => {
                    // if the operation didn't exceed the limit then apply it
                    // to the oracle
                    if subject.$operation(range.clone()).is_ok() {
                        oracle.$operation(range.clone());
                        subject.$operation(range.clone()).unwrap();
                    }
                }
                RangeValue::RangeInclusive(range) => {
                    // if the operation didn't exceed the limit then apply it
                    // to the oracle
                    if subject.$operation(range.clone()).is_ok() {
                        oracle.$operation(range.clone());
                        subject.$operation(range.clone()).unwrap();
                    }
                }
            }
        }};
    }

    for operation in operations {
        match operation {
            Operation::Insert { range } => {
                perform_operation!(insert, range);
            }
            Operation::Remove { range } => {
                perform_operation!(remove, range);
            }
        }

        if let Some(limit) = limit {
            assert!(
                subject.interval_len() <= limit.get(),
                "lhs: {}, rhs: {}",
                subject.interval_len(),
                limit
            );
        }
    }

    assert_set_eq!(oracle, subject);

    (oracle, subject)
}

/// `Oracle` is modeled as a `BTreeSet`. Instead of storing `Interval`s,
/// each number contained inside the `Interval` is inserted into
/// the set. This is semantically equivalent, but much less efficient.
/// We can assert that given the same operations, the two implementations
/// should result in the same values.
#[derive(Debug)]
struct Oracle {
    data: BTreeSet<RangeBound>,
}

impl Oracle {
    fn new() -> Self {
        Oracle {
            data: Default::default(),
        }
    }

    fn insert<V: Iterator<Item = RangeBound>>(&mut self, values: V) {
        for value in values {
            self.data.insert(value);
        }
    }

    fn remove<V: Iterator<Item = RangeBound>>(&mut self, values: V) {
        for value in values {
            self.data.remove(&value);
        }
    }

    fn iter(&self) -> impl Iterator<Item = RangeBound> + '_ {
        self.data.iter().cloned()
    }

    fn difference(&mut self, other: &Self) {
        self.data = self.data.difference(&other.data).cloned().collect();
    }

    fn union(&mut self, other: &Self) {
        self.data = self.data.union(&other.data).cloned().collect();
    }

    fn intersection(&mut self, other: &Self) {
        self.data = self.data.intersection(&other.data).cloned().collect();
    }

    fn count(&self) -> usize {
        self.data.len()
    }

    fn interval_len(&self) -> usize {
        if self.data.is_empty() {
            return 0;
        }

        let mut iter = self.iter();
        let mut prev = iter.next().unwrap();
        let mut count = 1;

        for item in iter {
            if item != prev + 1 {
                count += 1;
            }
            prev = item;
        }

        count
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{cell::Cell, fmt};

/// A datastructure that [memoizes](https://wikipedia.org/wiki/Memoization) a query function
///
/// This can be used for when queries rarely change and can potentially be expensive or on hot
/// code paths. After the `input` is mutated, the query value should be `clear`ed to signal that
/// the function needs to be executed again.
///
/// In debug mode the `get` call will always run the query and assert that the values match.
#[derive(Clone)]
pub struct Memo<T: Copy, Input, Check = DefaultConsistencyCheck> {
    value: Cell<Option<T>>,
    query: fn(&Input) -> T,
    check: Check,
}

impl<T: Copy + fmt::Debug, Input, Check> fmt::Debug for Memo<T, Input, Check> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Memo").field(&self.value.get()).finish()
    }
}

impl<T: Copy + PartialEq + fmt::Debug, Input, Check: ConsistencyCheck> Memo<T, Input, Check> {
    /// Creates a new `Memo` over a query function
    #[inline]
    pub fn new(query: fn(&Input) -> T) -> Self {
        Self {
            value: Cell::new(None),
            query,
            check: Check::default(),
        }
    }

    /// Returns the current value of the query function, which may be cached
    #[inline]
    #[track_caller]
    pub fn get(&self, input: &Input) -> T {
        if let Some(value) = self.value.get() {
            // make sure the values match
            self.check.check_consistency(value, input, self.query);
            return value;
        }

        let value = (self.query)(input);
        self.value.set(Some(value));
        value
    }

    /// Clears the cached value of the query function
    #[inline]
    pub fn clear(&self) {
        self.value.set(None);
    }

    /// Asserts that the cached value reflects the current query result in debug mode
    #[inline]
    #[track_caller]
    pub fn check_consistency(&self, input: &Input) {
        if cfg!(debug_assertions) {
            // `get` will assert the value matches the query internally
            let _ = self.get(input);
        }
    }
}

/// Trait to configure consistency checking behavior
pub trait ConsistencyCheck: Clone + Default {
    /// Called when the `Memo` struct has a cached value
    ///
    /// An implementation can assert that the `cache` value matches the current `query` result
    fn check_consistency<T: PartialEq + fmt::Debug, Input>(
        &self,
        cache: T,
        input: &Input,
        query: fn(&Input) -> T,
    );
}

#[derive(Copy, Clone, Default)]
pub struct ConsistencyCheckAlways;

impl ConsistencyCheck for ConsistencyCheckAlways {
    #[inline]
    fn check_consistency<T: PartialEq + fmt::Debug, Input>(
        &self,
        actual: T,
        input: &Input,
        query: fn(&Input) -> T,
    ) {
        let expected = query(input);
        assert_eq!(expected, actual);
    }
}

#[derive(Copy, Clone, Default)]
pub struct ConsistencyCheckNever;

impl ConsistencyCheck for ConsistencyCheckNever {
    #[inline]
    fn check_consistency<T: PartialEq + fmt::Debug, Input>(
        &self,
        _cache: T,
        _input: &Input,
        _query: fn(&Input) -> T,
    ) {
        // noop
    }
}

#[cfg(debug_assertions)]
pub type DefaultConsistencyCheck = ConsistencyCheckAlways;
#[cfg(not(debug_assertions))]
pub type DefaultConsistencyCheck = ConsistencyCheckNever;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct Input<Value> {
        value: Value,
        should_query: bool,
    }

    #[test]
    fn memo_test() {
        let memo = Memo::<u64, Input<_>, ConsistencyCheckNever>::new(|input| {
            assert!(
                input.should_query,
                "query was called when it wasn't expected"
            );
            input.value
        });

        assert_eq!(
            memo.get(&Input {
                value: 1,
                should_query: true,
            }),
            1
        );

        assert_eq!(
            memo.get(&Input {
                value: 2,
                should_query: false,
            }),
            1
        );

        memo.clear();

        assert_eq!(
            memo.get(&Input {
                value: 3,
                should_query: true,
            }),
            3
        );

        assert_eq!(
            memo.get(&Input {
                value: 4,
                should_query: false,
            }),
            3
        );
    }
}

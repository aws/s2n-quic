// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::number::{
    CheckedAddAssign, CheckedMulAssign, CheckedSubAssign, SaturatingAddAssign, SaturatingMulAssign,
    SaturatingSubAssign, UpcastFrom,
};
use core::{cmp::Ordering, marker::PhantomData, ops};

/// A checked-overflow counter
///
/// Rather than silently wrapping, we want to ensure counting errors stay somewhat isolated so the
/// counter will saturate rather than wrap. The counter operates in 3 modes:
///
/// * If `debug_assertions` are enabled, the counter will panic on overflow
/// * If the `checked-counters` feature flag is defined, the counter will panic on overflow, even in
///   release builds.
/// * Otherwise, the counter will saturate
///
/// The counter can also be configured to always saturate by passing the `Saturating` behavior:
///
/// ```rust
/// use s2n_quic_core::counter::{Counter, Saturating};
///
/// let counter: Counter<u32, Saturating> = Default::default();
/// ```
#[derive(Clone, Copy, Debug, Default, Hash)]
pub struct Counter<T, Behavior = ()>(T, PhantomData<Behavior>);

/// Overrides the behavior of a counter to always saturate
#[derive(Clone, Copy, Debug, Default, Hash)]
pub struct Saturating;

impl<T, Behavior> Counter<T, Behavior> {
    /// Creates a new counter with an initial value
    #[inline]
    pub const fn new(value: T) -> Self {
        Self(value, PhantomData)
    }

    #[inline]
    pub fn set(&mut self, value: T) {
        self.0 = value;
    }

    /// Tries to convert V to T and add it to the current counter value
    #[inline]
    pub fn try_add<V>(&mut self, value: V) -> Result<(), T::Error>
    where
        T: TryFrom<V>,
        Self: ops::AddAssign<T>,
    {
        let value = T::try_from(value)?;
        *self += value;
        Ok(())
    }

    /// Tries to convert V to T and subtract it from the current counter value
    #[inline]
    pub fn try_sub<V>(&mut self, value: V) -> Result<(), T::Error>
    where
        T: TryFrom<V>,
        Self: ops::SubAssign<T>,
    {
        let value = T::try_from(value)?;
        *self -= value;
        Ok(())
    }
}

/// Generates an assign trait implementation for the Counter
macro_rules! assign_trait {
    (
        $op:ident,
        $method:ident,
        $saturating_trait:ident,
        $saturating_method:ident,
        $checked_trait:ident,
        $checked:ident
    ) => {
        impl<T, R> ops::$op<R> for Counter<T, ()>
        where
            T: $saturating_trait<R> + $checked_trait<R> + ops::$op + UpcastFrom<R>,
        {
            #[inline]
            fn $method(&mut self, rhs: R) {
                if cfg!(feature = "checked-counters") {
                    (self.0).$checked(rhs).expect("counter overflow");
                } else if cfg!(debug_assertions) {
                    (self.0).$method(T::upcast_from(rhs));
                } else {
                    (self.0).$saturating_method(rhs);
                }
            }
        }

        impl<T, R> ops::$op<R> for Counter<T, Saturating>
        where
            T: $saturating_trait<R>,
        {
            #[inline]
            fn $method(&mut self, rhs: R) {
                (self.0).$saturating_method(rhs);
            }
        }
    };
}

assign_trait!(
    AddAssign,
    add_assign,
    SaturatingAddAssign,
    saturating_add_assign,
    CheckedAddAssign,
    checked_add_assign
);

assign_trait!(
    SubAssign,
    sub_assign,
    SaturatingSubAssign,
    saturating_sub_assign,
    CheckedSubAssign,
    checked_sub_assign
);

assign_trait!(
    MulAssign,
    mul_assign,
    SaturatingMulAssign,
    saturating_mul_assign,
    CheckedMulAssign,
    checked_mul_assign
);

impl<T, B> UpcastFrom<Counter<T, B>> for T {
    #[inline]
    fn upcast_from(value: Counter<T, B>) -> Self {
        value.0
    }
}

impl<T, B> UpcastFrom<&Counter<T, B>> for T
where
    T: for<'a> UpcastFrom<&'a T>,
{
    #[inline]
    fn upcast_from(value: &Counter<T, B>) -> Self {
        T::upcast_from(&value.0)
    }
}

impl<T, B> ops::Deref for Counter<T, B> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, B, R> PartialEq<R> for Counter<T, B>
where
    Self: PartialOrd<R>,
{
    #[inline]
    fn eq(&self, other: &R) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

impl<T, B> PartialOrd<T> for Counter<T, B>
where
    T: PartialOrd<T>,
{
    #[inline]
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        self.0.partial_cmp(other)
    }
}

impl<T, B> PartialOrd for Counter<T, B>
where
    T: PartialOrd<T>,
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T, B> Eq for Counter<T, B> where Self: Ord {}

impl<T, B> Ord for Counter<T, B>
where
    T: Ord,
{
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_upcast() {
        let mut a: Counter<u32> = Counter::new(0);
        a += 1u8;
        a += 2u16;
        a += 3u32;

        assert_eq!(a, Counter::new(6));
        assert_eq!(a, 6u32);
    }

    #[test]
    fn saturating() {
        let mut a: Counter<u8, Saturating> = Counter::new(0);
        a += 250;
        a += 250;
        a += 123;

        assert_eq!(a, Counter::new(255));
    }
}

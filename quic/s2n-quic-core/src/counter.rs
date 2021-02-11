use crate::number::{
    CheckedAddAssign, CheckedSubAssign, SaturatingAddAssign, SaturatingSubAssign, UpcastFrom,
};
use core::{cmp::Ordering, convert::TryFrom, marker::PhantomData, ops};

/// A counter that panics on overflow and saturates rather than wraps without debug_assertions
///
/// Rather than silently wrapping, we want to ensure counting errors stay somewhat isolated so the
/// counter will saturate rather than wrap. The `checked-counters` feature flag can be passed in
/// order to always check for overflow.
#[derive(Clone, Copy, Debug, Default, Hash)]
pub struct Counter<T, Behavior = ()>(T, PhantomData<Behavior>);

/// Overrides the behavior of a counter to always saturate
#[derive(Clone, Copy, Debug, Default, Hash)]
struct Saturating;

impl<T, Behavior> Counter<T, Behavior> {
    /// Creates a new counter with an initial value
    pub const fn new(value: T) -> Self {
        Self(value, PhantomData)
    }

    /// Tries to convert V to T and add it to the current counter value
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

impl<T, B> UpcastFrom<Counter<T, B>> for T {
    fn upcast_from(value: Counter<T, B>) -> Self {
        value.0
    }
}

impl<T, B> UpcastFrom<&Counter<T, B>> for T
where
    T: for<'a> UpcastFrom<&'a T>,
{
    fn upcast_from(value: &Counter<T, B>) -> Self {
        T::upcast_from(&value.0)
    }
}

impl<T, B> ops::Deref for Counter<T, B> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, B, R> PartialEq<R> for Counter<T, B>
where
    Self: PartialOrd<R>,
{
    fn eq(&self, other: &R) -> bool {
        self.partial_cmp(&other) == Some(Ordering::Equal)
    }
}

impl<T, B> PartialOrd<T> for Counter<T, B>
where
    T: PartialOrd<T>,
{
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        self.0.partial_cmp(other)
    }
}

impl<T, B> PartialOrd for Counter<T, B>
where
    T: PartialOrd<T>,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl<T, B> Eq for Counter<T, B> where Self: Ord {}

impl<T, B> Ord for Counter<T, B>
where
    T: Ord,
{
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

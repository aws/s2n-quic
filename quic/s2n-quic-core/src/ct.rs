// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops;
use num_traits::ops::{
    checked::{CheckedDiv, CheckedRem, CheckedShl, CheckedShr},
    overflowing::{OverflowingAdd, OverflowingMul, OverflowingSub},
};
pub use subtle::{Choice, ConditionallySelectable, ConstantTimeEq, CtOption};

/// A best-effort constant-time number used for reducing branching
/// based on secret information
#[derive(Copy, Clone, Debug)]
pub struct Number<T>(CtOption<T>);

impl<T> Number<T> {
    pub fn new(value: T) -> Self {
        Self(CtOption::new(value, Choice::from(1u8)))
    }

    pub fn is_valid(&self) -> Choice {
        self.0.is_some()
    }

    // See https://github.com/rust-lang/rust-clippy/issues/11390
    #[allow(clippy::unwrap_or_default)]
    pub fn unwrap_or_default(&self) -> T
    where
        T: ConditionallySelectable + Default,
    {
        self.0.unwrap_or_else(Default::default)
    }

    pub fn and_then<U, F, C>(self, f: F) -> Number<U>
    where
        T: ConditionallySelectable + Default,
        F: FnOnce(T) -> (U, C),
        C: Into<Choice>,
    {
        Number(self.0.and_then(|value| {
            let (next, is_valid) = f(value);
            CtOption::new(next, is_valid.into())
        }))
    }

    #[must_use]
    pub fn filter<F, C>(self, f: F) -> Self
    where
        T: ConditionallySelectable + Default,
        F: FnOnce(T) -> C,
        C: Into<Choice>,
    {
        Number(self.0.and_then(|value| {
            let is_valid = f(value);
            CtOption::new(value, is_valid.into())
        }))
    }

    pub fn ct_lt(self, rhs: Self) -> Choice
    where
        T: ConditionallySelectable + Default + OverflowingSub,
    {
        (self - rhs).0.is_none()
    }

    pub fn ct_le(self, rhs: Self) -> Choice
    where
        T: ConditionallySelectable + Default + OverflowingSub,
    {
        (rhs - self).0.is_some()
    }

    pub fn ct_ge(self, rhs: Self) -> Choice
    where
        T: ConditionallySelectable + Default + OverflowingSub,
    {
        (self - rhs).0.is_some()
    }

    pub fn ct_gt(self, rhs: Self) -> Choice
    where
        T: ConditionallySelectable + Default + OverflowingSub,
    {
        (rhs - self).0.is_none()
    }
}

impl<T> From<T> for Number<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> ConditionallySelectable for Number<T>
where
    T: ConditionallySelectable,
{
    fn conditional_select(a: &Self, b: &Self, choice: Choice) -> Self {
        Self(CtOption::conditional_select(&a.0, &b.0, choice))
    }
}

impl<T> ConstantTimeEq for Number<T>
where
    T: ConstantTimeEq,
{
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)
    }
}

impl<T> ops::Add for Number<T>
where
    T: ConditionallySelectable + Default + OverflowingAdd,
{
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self + rhs).0))
    }
}

impl<T> ops::Add<T> for Number<T>
where
    T: ConditionallySelectable + Default + OverflowingAdd,
{
    type Output = Self;

    fn add(self, rhs: T) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let (next, overflowed) = prev.overflowing_add(&rhs);
            let is_valid = !overflowed as u8;
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Sub for Number<T>
where
    T: ConditionallySelectable + Default + OverflowingSub,
{
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self - rhs).0))
    }
}

impl<T> ops::Sub<T> for Number<T>
where
    T: ConditionallySelectable + Default + OverflowingSub,
{
    type Output = Self;

    fn sub(self, rhs: T) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let (next, overflowed) = prev.overflowing_sub(&rhs);
            let is_valid = !overflowed as u8;
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Mul for Number<T>
where
    T: ConditionallySelectable + Default + OverflowingMul,
{
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self * rhs).0))
    }
}

impl<T> ops::Mul<T> for Number<T>
where
    T: ConditionallySelectable + Default + OverflowingMul,
{
    type Output = Self;

    fn mul(self, rhs: T) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let (next, overflowed) = prev.overflowing_mul(&rhs);
            let is_valid = !overflowed as u8;
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Div for Number<T>
where
    T: ConditionallySelectable + Default + CheckedDiv,
{
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self / rhs).0))
    }
}

impl<T> ops::Div<T> for Number<T>
where
    T: ConditionallySelectable + Default + CheckedDiv,
{
    type Output = Self;

    fn div(self, rhs: T) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let next = prev.checked_div(&rhs);
            let is_valid = next.is_some() as u8;
            let next = next.unwrap_or_default();
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Rem for Number<T>
where
    T: ConditionallySelectable + Default + CheckedRem,
{
    type Output = Self;

    fn rem(self, rhs: Self) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self % rhs).0))
    }
}

impl<T> ops::Rem<T> for Number<T>
where
    T: ConditionallySelectable + Default + CheckedRem,
{
    type Output = Self;

    fn rem(self, rhs: T) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let next = prev.checked_rem(&rhs);
            let is_valid = next.is_some() as u8;
            let next = next.unwrap_or_default();
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Shl<Number<u32>> for Number<T>
where
    T: ConditionallySelectable + Default + CheckedShl,
{
    type Output = Self;

    fn shl(self, rhs: Number<u32>) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self << rhs).0))
    }
}

impl<T> ops::Shl<u32> for Number<T>
where
    T: ConditionallySelectable + Default + CheckedShl,
{
    type Output = Self;

    fn shl(self, rhs: u32) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let next = prev.checked_shl(rhs);
            let is_valid = next.is_some() as u8;
            let next = next.unwrap_or_default();
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Shr<Number<u32>> for Number<T>
where
    T: ConditionallySelectable + Default + CheckedShr,
{
    type Output = Self;

    fn shr(self, rhs: Number<u32>) -> Self::Output {
        Self(rhs.0.and_then(|rhs| (self >> rhs).0))
    }
}

impl<T> ops::Shr<u32> for Number<T>
where
    T: ConditionallySelectable + Default + CheckedShr,
{
    type Output = Self;

    fn shr(self, rhs: u32) -> Self::Output {
        Self(self.0.and_then(|prev| {
            let next = prev.checked_shr(rhs);
            let is_valid = next.is_some() as u8;
            let next = next.unwrap_or_default();
            CtOption::new(next, is_valid.into())
        }))
    }
}

impl<T> ops::Not for Number<T>
where
    T: ConditionallySelectable + Default + ops::Not,
{
    type Output = Number<T::Output>;

    fn not(self) -> Self::Output {
        Number(self.0.map(|prev| prev.not()))
    }
}

impl<T> ops::BitAnd for Number<T>
where
    T: ConditionallySelectable + Default + ops::BitAnd,
{
    type Output = Number<T::Output>;

    fn bitand(self, rhs: Self) -> Self::Output {
        Number(self.0.and_then(|prev| rhs.0.map(|rhs| prev.bitand(rhs))))
    }
}

impl<T> ops::BitOr for Number<T>
where
    T: ConditionallySelectable + Default + ops::BitOr,
{
    type Output = Number<T::Output>;

    fn bitor(self, rhs: Self) -> Self::Output {
        Number(self.0.and_then(|prev| rhs.0.map(|rhs| prev.bitor(rhs))))
    }
}

impl<T> ops::BitXor for Number<T>
where
    T: ConditionallySelectable + Default + ops::BitXor,
{
    type Output = Number<T::Output>;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Number(self.0.and_then(|prev| rhs.0.map(|rhs| prev.bitxor(rhs))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use ops::*;

    macro_rules! binop_test {
        ($op:ident, $checked_op:ident) => {
            #[test]
            #[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
            fn $op() {
                check!()
                    .with_type::<(u8, u8)>()
                    .cloned()
                    .for_each(|(a, b)| {
                        let actual = Number::new(a).$op(Number::new(b)).unwrap_or_default();
                        if let Some(expected) = a.$checked_op(b) {
                            assert_eq!(actual, expected);
                        } else {
                            assert_eq!(actual, 0);
                        }
                    });
            }
        };
    }

    binop_test!(add, checked_add);
    binop_test!(sub, checked_sub);
    binop_test!(mul, checked_mul);
    binop_test!(div, checked_div);
    binop_test!(rem, checked_rem);

    macro_rules! cmp_test {
        ($op:ident, $core_op:ident) => {
            #[test]
            #[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
            fn $op() {
                check!()
                    .with_type::<(u8, u8)>()
                    .cloned()
                    .for_each(|(a, b)| {
                        let actual: bool = Number::new(a).$op(Number::new(b)).into();
                        let expected = a.$core_op(&b);
                        assert_eq!(actual, expected);
                    });
            }
        };
    }

    cmp_test!(ct_lt, lt);
    cmp_test!(ct_le, le);
    cmp_test!(ct_gt, gt);
    cmp_test!(ct_ge, ge);
}

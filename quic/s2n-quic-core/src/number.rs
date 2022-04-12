// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Generates a generic number operation
macro_rules! number_trait {
    ($name:ident, $method:ident, $output:ty, $test:stmt) => {
        number_trait!($name, $method, $output, $test, [
            u8, i8, u16, i16, u32, i32, u64, i64, usize, isize, u128, i128
        ]);
    };
    ($name:ident, $method:ident, $output:ty, $test:stmt, [$($ty:ident),*]) => {
        pub trait $name<Rhs = Self>: Sized {
            type Output;
            fn $method(self, rhs: Rhs) -> $output;
        }

        $(
           impl<Rhs> $name<Rhs> for $ty
                where $ty: UpcastFrom<Rhs>
           {
               type Output = Self;

               fn $method(self, rhs: Rhs) -> $output {
                   $ty::$method(self, rhs.upcast())
               }
           }
        )*

        #[test]
        fn $method() {
            $({
                type Type = $ty;
                $test
            })*
        }
    };
}

/// Generates a generic assign operation trait
macro_rules! assign_trait {
    ($name:ident, $method:ident, $delegate:expr, $output:ty) => {
        assign_trait!($name, $method, $delegate, $output, [
            u8, i8, u16, i16, u32, i32, u64, i64, usize, isize, u128, i128
        ]);
    };
    ($name:ident, $method:ident, $delegate:expr, $output:ty, [$($ty:ident),*]) => {
        pub trait $name<Rhs = Self> {
            fn $method(&mut self, rhs: Rhs) -> $output;
        }

        $(
           impl<Rhs> $name<Rhs> for $ty
             where $ty: UpcastFrom<Rhs>
           {
               fn $method(&mut self, rhs: Rhs) -> $output {
                   let f: fn(&mut $ty, Rhs) -> $output = $delegate;
                   f(self, rhs)
               }
           }
        )*
    };
}

number_trait!(
    CheckedAdd,
    checked_add,
    Option<Self::Output>,
    assert!(Type::MAX.checked_add(Type::MAX).is_none())
);

number_trait!(
    CheckedSub,
    checked_sub,
    Option<Self::Output>,
    assert!(Type::MIN.checked_sub(Type::MAX).is_none())
);

number_trait!(
    CheckedMul,
    checked_mul,
    Option<Self::Output>,
    assert!(Type::MAX.checked_mul(Type::MAX).is_none())
);

number_trait!(
    CheckedDiv,
    checked_div,
    Option<Self::Output>,
    assert!(Type::MAX.checked_div(0).is_none())
);

assign_trait!(
    CheckedAddAssign,
    checked_add_assign,
    |value, rhs| {
        *value = CheckedAdd::checked_add(*value, rhs)?;
        Some(())
    },
    Option<()>
);

assign_trait!(
    CheckedSubAssign,
    checked_sub_assign,
    |value, rhs| {
        *value = CheckedSub::checked_sub(*value, rhs)?;
        Some(())
    },
    Option<()>
);

assign_trait!(
    CheckedMulAssign,
    checked_mul_assign,
    |value, rhs| {
        *value = CheckedMul::checked_mul(*value, rhs)?;
        Some(())
    },
    Option<()>
);

number_trait!(
    SaturatingAdd,
    saturating_add,
    Self::Output,
    assert_eq!(Type::MAX.saturating_add(Type::MAX), Type::MAX)
);

number_trait!(
    SaturatingSub,
    saturating_sub,
    Self::Output,
    assert_eq!(Type::MIN.saturating_sub(Type::MAX), Type::MIN)
);

number_trait!(
    SaturatingMul,
    saturating_mul,
    Self::Output,
    assert_eq!(Type::MAX.saturating_mul(Type::MAX), Type::MAX)
);

assign_trait!(
    SaturatingAddAssign,
    saturating_add_assign,
    |value, rhs| {
        *value = SaturatingAdd::saturating_add(*value, rhs);
    },
    ()
);

assign_trait!(
    SaturatingSubAssign,
    saturating_sub_assign,
    |value, rhs| {
        *value = SaturatingSub::saturating_sub(*value, rhs);
    },
    ()
);

assign_trait!(
    SaturatingMulAssign,
    saturating_mul_assign,
    |value, rhs| {
        *value = SaturatingMul::saturating_mul(*value, rhs);
    },
    ()
);

/// Losslessly upcasts from one type to another
pub trait UpcastFrom<T> {
    fn upcast_from(value: T) -> Self;
}

/// Losslessly upcasts one type into another
pub trait Upcast<T> {
    fn upcast(self) -> T;
}

/// Implement Upcast automatically for all types that implement UpcastFrom
impl<T, U> Upcast<T> for U
where
    T: UpcastFrom<U>,
{
    fn upcast(self) -> T {
        UpcastFrom::upcast_from(self)
    }
}

macro_rules! upcast_impl {
    ($($ty:ident),*) => {
        upcast_impl!(@impl [], [$($ty),*]);
    };
    (@impl [$($prev:ident),*], []) => {
        // done!
    };
    (@impl [$($prev:ident),*], [$current:ident $(, $rest:ident)*]) => {
        impl UpcastFrom<$current> for $current {
            fn upcast_from(value: Self) -> Self {
                value
            }
        }

        impl UpcastFrom<&$current> for $current {
            fn upcast_from(value: &Self) -> Self {
                *value
            }
        }

        $(
            impl UpcastFrom<$prev> for $current {
                fn upcast_from(value: $prev) -> Self {
                    value as $current
                }
            }

            impl UpcastFrom<&$prev> for $current {
                fn upcast_from(value: &$prev) -> Self {
                    (*value) as $current
                }
            }
        )*

        upcast_impl!(@impl [$current $(, $prev)*], [$($rest),*]);
    };
}

upcast_impl!(u8, u16, u32, u64, u128);
upcast_impl!(i8, i16, i32, i64, i128);

macro_rules! upcast_usize {
    ($target_width:literal, $unsigned:ident, $signed:ident) => {
        #[cfg(target_pointer_width = $target_width)]
        impl<Rhs> UpcastFrom<Rhs> for usize
        where
            $unsigned: UpcastFrom<Rhs>,
        {
            fn upcast_from(value: Rhs) -> Self {
                $unsigned::upcast_from(value) as usize
            }
        }

        #[cfg(target_pointer_width = $target_width)]
        impl<Rhs> UpcastFrom<Rhs> for isize
        where
            $signed: UpcastFrom<Rhs>,
        {
            fn upcast_from(value: Rhs) -> Self {
                $signed::upcast_from(value) as isize
            }
        }
    };
}

upcast_usize!("8", u8, i8);
upcast_usize!("16", u16, u16);
upcast_usize!("32", u32, i32);
upcast_usize!("64", u64, i64);
upcast_usize!("128", u128, i128);

/// A rational number represented by a numerator and a denominator
pub struct Fraction(u32, u32);

impl Fraction {
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        Self(numerator, denominator)
    }

    pub fn numerator(&self) -> u32 {
        self.0
    }

    pub fn denominator(&self) -> u32 {
        self.1
    }
}

impl core::ops::Div<Fraction> for core::time::Duration {
    type Output = core::time::Duration;

    fn div(self, rhs: Fraction) -> Self::Output {
        self * rhs.1 / rhs.0
    }
}

impl core::ops::Mul<Fraction> for u32 {
    type Output = u32;

    fn mul(self, rhs: Fraction) -> Self::Output {
        self * rhs.0 / rhs.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[test]
    fn duration_div() {
        let duration = Duration::from_millis(50000);
        let fraction = Fraction::new(10, 5);

        let result = duration / fraction;

        assert_eq!(result, duration / 2);
    }

    #[test]
    fn u32_mul() {
        let num = 7000;
        let fraction = Fraction::new(3, 7);

        let result = num * fraction;

        assert_eq!(result, 3000);
    }
}

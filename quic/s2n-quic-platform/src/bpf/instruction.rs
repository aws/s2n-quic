// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, marker::PhantomData, ops};
use libc::sock_filter;

pub trait Dialect: Sized {
    const MAX_INSTRUCTIONS: usize;
    const SOCKOPT: libc::c_int;

    fn debug(instruction: &Instruction<Self>, f: &mut fmt::Formatter) -> fmt::Result;
    fn display(
        instruction: &Instruction<Self>,
        f: &mut fmt::Formatter,
        idx: Option<usize>,
    ) -> fmt::Result;
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Instruction<D: Dialect>(sock_filter, PhantomData<D>);

impl<D: Dialect> ops::Deref for Instruction<D> {
    type Target = sock_filter;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<D: Dialect> fmt::Debug for Instruction<D> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        D::debug(self, f)
    }
}

impl<D: Dialect> fmt::Display for Instruction<D> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if f.alternate() {
            // write the C literal format if requested
            let k_prefix = if self.k == 0 { "00" } else { "0x" };
            write!(
                f,
                "{{ 0x{:0>2x}, {:>2}, {:>2}, {k_prefix}{:0>8x} }}",
                self.code, self.jt, self.jf, self.k
            )
        } else {
            D::display(self, f, None)
        }
    }
}

macro_rules! define {
    (#[mask($mask:literal)]
     pub enum $ty:ident {
        $(
            $OP:ident = $value:literal
        ),*
        $(,)?
    }) => {
        #[repr(u16)]
        #[derive(Copy, Clone, Debug)]
        #[allow(clippy::upper_case_acronyms, non_camel_case_types)]
        pub enum $ty {
            $(
                $OP = $value,
            )*
        }

        impl core::fmt::Display for $ty {
            #[inline]
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                self.to_str().fmt(f)
            }
        }

        impl $ty {
            #[inline]
            pub fn decode(op: u16) -> Self {
                match op & $mask {
                    $(
                        op if op == $value => Self::$OP,
                    )*
                    _ => unreachable!(),
                }
            }

            #[inline]
            pub const fn to_str(self) -> &'static str {
                match self {
                    $(
                        Self::$OP => stringify!($OP),
                    )*
                }
            }
        }
    }
}

impl<D: Dialect> Instruction<D> {
    pub const fn raw(code: u16, jt: u8, jf: u8, k: u32) -> Self {
        Instruction(libc::sock_filter { code, jt, jf, k }, PhantomData)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

/// Asserts that a boolean expression is true at runtime, only if debug_assertions are enabled.
///
/// Otherwise, the compiler is told to assume that the expression is always true and can perform
/// additional optimizations.
///
/// # Safety
///
/// The caller _must_ ensure this condition is never possible, otherwise the compiler
/// may optimize based on false assumptions and behave incorrectly.
#[macro_export]
macro_rules! assume {
    ($cond:expr) => {
        $crate::assume!($cond, "assumption failed: {}", stringify!($cond));
    };
    ($cond:expr $(, $fmtarg:expr)* $(,)?) => {
        let v = $cond;

        debug_assert!(v $(, $fmtarg)*);
        if cfg!(not(debug_assertions)) && !v {
            core::hint::unreachable_unchecked();
        }
    };
}

#[doc(hidden)]
#[cfg(feature = "branch-tracing")]
pub use tracing::trace as _trace;

#[cfg(not(feature = "branch-tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! _trace {
    ($($fmt:tt)*) => {};
}

/// Traces a branch value if the `branch-tracing` feature is enabled. Otherwise it is a no-op.
#[macro_export]
macro_rules! branch {
    ($cond:expr) => {{
        let result = $cond;
        $crate::_trace!(
            branch = stringify!($cond),
            result,
            file = file!(),
            line = line!(),
        );
        result
    }};
    ($cond:expr, $outcome:expr) => {
        $crate::branch!($cond, stringify!($cond), stringify!($outcome))
    };
    ($cond:expr, $branch:expr, $outcome:expr) => {{
        let result = $cond;
        if result {
            $crate::_trace!(branch = $branch, result, file = file!(), line = line!(),);
        } else {
            $crate::_trace!(
                branch = $branch,
                result,
                outcome = $outcome,
                file = file!(),
                line = line!(),
            );
        }
        result
    }};
}

#[macro_export]
macro_rules! ready {
    ($expr:expr) => {
        match $expr {
            ::core::task::Poll::Pending => {
                $crate::branch!(false, stringify!($expr), "return Poll::Pending");
                return ::core::task::Poll::Pending;
            }
            ::core::task::Poll::Ready(v) => {
                $crate::branch!(true, stringify!($expr), "");
                v
            }
        }
    };
}

/// Checks that the first argument is true, otherwise returns the second value
#[macro_export]
macro_rules! ensure {
    (let $pat:pat = $expr:expr, continue) => {
        let $pat = $expr else {
            $crate::branch!(false, stringify!(let $pat = $expr), stringify!(continue));
            continue;
        };
        $crate::branch!(true, stringify!(let $pat = $expr), "");
    };
    (let $pat:pat = $expr:expr, break $($ret:expr)?) => {
        let $pat = $expr else {
            $crate::branch!(false, stringify!(let $pat = $expr), stringify!(break $($ret)?));
            break $($ret)?;
        };
        $crate::branch!(true, stringify!(let $pat = $expr), "");
    };
    (let $pat:pat = $expr:expr, return $($ret:expr)?) => {
        let $pat = $expr else {
            $crate::branch!(false, stringify!(let $pat = $expr), stringify!(return $($ret)?));
            return $($ret)?;
        };
        $crate::branch!(true, stringify!(let $pat = $expr), "");
    };
    (let $pat:pat = $expr:expr $(, $ret:expr)?) => {
        $crate::ensure!(let $pat = $expr, return $($ret)?)
    };
    ($cond:expr, continue) => {
        if !$crate::branch!($cond, continue) {
            continue;
        }
    };
    ($cond:expr, break $($expr:expr)?) => {
        if !$crate::branch!($cond, break $($expr)?) {
            break $($expr)?;
        }
    };
    ($cond:expr, return $($expr:expr)?) => {
        if !$crate::branch!($cond, return $($expr)?) {
            return $($expr)?;
        }
    };
    ($cond:expr $(, $ret:expr)?) => {
        $crate::ensure!($cond, return $($ret)?);
    };
}

/// Implements a future that wraps `T::poll_ready` and yields after ready
macro_rules! impl_ready_future {
    ($name:ident, $fut:ident, $output:ty) => {
        pub struct $fut<'a, T>(&'a mut T);

        impl<'a, T: $name> core::future::Future for $fut<'a, T> {
            type Output = $output;

            #[inline]
            fn poll(
                mut self: core::pin::Pin<&mut Self>,
                cx: &mut core::task::Context,
            ) -> core::task::Poll<Self::Output> {
                self.0.poll_ready(cx)
            }
        }
    };
}

#[macro_use]
pub mod probe;

pub mod ack;
pub mod application;
#[cfg(feature = "alloc")]
pub mod buffer;
pub mod connection;
pub mod counter;
pub mod crypto;
pub mod ct;
pub mod datagram;
pub mod endpoint;
pub mod event;
pub mod frame;
pub mod havoc;
pub mod inet;
#[cfg(feature = "alloc")]
pub mod interval_set;
pub mod io;
pub mod memo;
pub mod number;
pub mod packet;
pub mod path;
pub mod query;
pub mod random;
pub mod recovery;
pub mod slice;
pub mod stateless_reset;
pub mod stream;
pub mod sync;
pub mod task;
pub mod time;
pub mod token;
pub mod transmission;
pub mod transport;
pub mod varint;
pub mod xdp;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

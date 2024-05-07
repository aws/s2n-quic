// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

pub type Result<T> = core::result::Result<(), Error<T>>;

#[cfg(feature = "state-tracing")]
#[doc(hidden)]
pub use tracing::debug as _debug;

#[cfg(not(feature = "state-tracing"))]
#[doc(hidden)]
pub use crate::__tracing_noop__ as _debug;

#[macro_export]
#[doc(hidden)]
macro_rules! __state_transition__ {
    ($state:ident, $valid:pat => $target:expr) => {
        $crate::state::transition!(_, $state, $valid => $target)
    };
    ($event:ident, $state:ident, $valid:pat => $target:expr) => {{
        if *$state == $target {
            // if transitioning to itself is not called out, then return an error
            $crate::ensure!(
                matches!($state, $valid),
                Err($crate::state::Error::NoOp { current: $target })
            );
            return Ok(());
        }

        // make sure the transition is valid
        $crate::ensure!(
            matches!($state, $valid),
            Err($crate::state::Error::InvalidTransition {
                current: $state.clone(),
                target: $target
            })
        );

        let __event__ = stringify!($event);
        if __event__.is_empty() || __event__ == "_" {
            $crate::state::_debug!(prev = ?$state, next = ?$target);
        } else {
            $crate::state::_debug!(event = %__event__, prev = ?$state, next = ?$target);
        }

        *$state = $target;
        Ok(())
    }};
}

pub use crate::__state_transition__ as transition;

#[macro_export]
#[doc(hidden)]
macro_rules! __state_event__ {
    ($(#[doc = $doc:literal])* $event:ident ($($valid:ident)|* => $target:ident)) => {
        $(
            #[doc = $doc]
        )*
        #[inline]
        pub fn $event(&mut self) -> $crate::state::Result<Self> {
            $crate::state::transition!($event, self, $(Self::$valid)|* => Self::$target)
        }
    };
    ($( $(#[doc = $doc:literal])* $event:ident ($($valid:ident)|* => $target:ident) ;)*) => {
        $(
            $crate::state::event!($(#[doc = $doc])* $event($($valid)|* => $target));
        )*

        /// Generates a dot graph of all state transitions
        pub fn dot() -> impl ::core::fmt::Display {
            struct Dot;

            impl ::core::fmt::Display for Dot {
                fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                    writeln!(f, "digraph {{")?;

                    let mut all_states = [
                        // collect all of the states we've observed
                        $(
                            $(
                                stringify!($valid),
                            )*
                            stringify!($target),
                        )*
                    ];

                    let all_states = $crate::state::dedup_states(&mut all_states);

                    for state in all_states {
                        writeln!(f, "  {state};")?;
                    }

                    $(
                        $(
                            writeln!(
                                f,
                                "  {} -> {} [label = {:?}];",
                                stringify!($valid),
                                stringify!($target),
                                stringify!($event),
                            )?;
                        )*
                    )*

                    writeln!(f, "}}")?;
                    Ok(())
                }
            }

            Dot
        }
    }
}

pub use crate::__state_event__ as event;

#[doc(hidden)]
pub fn dedup_states<'a>(states: &'a mut [&'a str]) -> &'a mut [&'a str] {
    // TODO replace with
    // https://doc.rust-lang.org/std/primitive.slice.html#method.partition_dedup
    // when stable
    //
    // For now, we've just inlined their implementation

    let len = states.len();
    if len <= 1 {
        return states;
    }

    // sort the states before deduping
    states.sort_unstable();

    let ptr = states.as_mut_ptr();
    let mut next_read: usize = 1;
    let mut next_write: usize = 1;

    // SAFETY: the `while` condition guarantees `next_read` and `next_write`
    // are less than `len`, thus are inside `self`. `prev_ptr_write` points to
    // one element before `ptr_write`, but `next_write` starts at 1, so
    // `prev_ptr_write` is never less than 0 and is inside the slice.
    // This fulfils the requirements for dereferencing `ptr_read`, `prev_ptr_write`
    // and `ptr_write`, and for using `ptr.add(next_read)`, `ptr.add(next_write - 1)`
    // and `prev_ptr_write.offset(1)`.
    //
    // `next_write` is also incremented at most once per loop at most meaning
    // no element is skipped when it may need to be swapped.
    //
    // `ptr_read` and `prev_ptr_write` never point to the same element. This
    // is required for `&mut *ptr_read`, `&mut *prev_ptr_write` to be safe.
    // The explanation is simply that `next_read >= next_write` is always true,
    // thus `next_read > next_write - 1` is too.
    unsafe {
        // Avoid bounds checks by using raw pointers.
        while next_read < len {
            let ptr_read = ptr.add(next_read);
            let prev_ptr_write = ptr.add(next_write - 1);
            if *ptr_read != *prev_ptr_write {
                if next_read != next_write {
                    let ptr_write = prev_ptr_write.add(1);
                    core::ptr::swap(ptr_read, ptr_write);
                }
                next_write += 1;
            }
            next_read += 1;
        }
    }

    &mut states[..next_write]
}

#[macro_export]
#[doc(hidden)]
macro_rules! __state_is__ {
    ($(#[doc = $doc:literal])* $function:ident, $($state:ident)|+) => {
        $(
            #[doc = $doc]
        )*
        #[inline]
        pub fn $function(&self) -> bool {
            matches!(self, $(Self::$state)|*)
        }
    };
}

pub use crate::__state_is__ as is;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<T> {
    NoOp { current: T },
    InvalidTransition { current: T, target: T },
}

impl<T: fmt::Debug> fmt::Display for Error<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOp { current } => {
                write!(f, "state is already set to {current:?}")
            }
            Self::InvalidTransition { current, target } => {
                write!(f, "invalid transition from {current:?} to {target:?}",)
            }
        }
    }
}

#[cfg(feature = "std")]
impl<T: fmt::Debug> std::error::Error for Error<T> {}

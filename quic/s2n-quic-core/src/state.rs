// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

#[cfg(test)]
mod tests;

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
        $crate::state::transition!(@build [], _, $state, [$valid => $target])
    };
    (@build [$($targets:expr),*], $event:ident, $state:ident, [$valid:pat => $target:expr] $($remaining:tt)*) => {{
        // if the transition is valid, then perform it
        if matches!($state, $valid) {
            let __event__ = stringify!($event);
            if __event__.is_empty() || __event__ == "_" {
                $crate::state::_debug!(prev = ?$state, next = ?$target);
            } else {
                $crate::state::_debug!(event = %__event__, prev = ?$state, next = ?$target);
            }

            *$state = $target;
            Ok(())
        } else {
            $crate::state::transition!(
                @build [$($targets,)* $target],
                $event,
                $state,
                $($remaining)*
            )
        }
    }};
    (@build [$($targets:expr),*], $event:ident, $state:ident $(,)?) => {{
        let targets = [$($targets),*];

        // if we only have a single target and the current state matches it, then return a no-op
        if targets.len() == 1 && targets[0].eq($state) {
            let current = targets[0].clone();
            Err($crate::state::Error::NoOp { current })
        } else {
            // if we didn't get a valid match then error out
            Err($crate::state::Error::InvalidTransition {
                current: $state.clone(),
                event: stringify!($event),
            })
        }
    }};
}

pub use crate::__state_transition__ as transition;

#[macro_export]
#[doc(hidden)]
macro_rules! __state_event__ {
    (
        $(#[doc = $doc:literal])*
        $event:ident (
            $(
                $($valid:ident)|* => $target:ident
            ),*
            $(,)?
        )
    ) => {
        $(
            #[doc = $doc]
        )*
        #[inline]
        pub fn $event(&mut self) -> $crate::state::Result<Self> {
            $crate::state::transition!(
                @build [],
                $event,
                self,
                $(
                    [$(Self::$valid)|* => Self::$target]
                )*
            )
        }
    };
    ($(
        $(#[doc = $doc:literal])*
        $event:ident (
            $(
                $($valid:ident)|* => $target:ident
            ),*
            $(,)?
        );
    )*) => {
        $(
            $crate::state::event!(
                $(#[doc = $doc])*
                $event($($($valid)|* => $target),*)
            );
        )*

        /// Generates a dot graph of all state transitions
        pub fn dot() -> impl ::core::fmt::Display {
            struct Dot;

            impl ::core::fmt::Display for Dot {
                fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                    writeln!(f, "digraph {{")?;

                    let mut all_states = [
                        // collect all of the states we've observed
                        $($(
                            $(
                                stringify!($valid),
                            )*
                            stringify!($target),
                        )*)*
                    ];

                    all_states.sort_unstable();
                    let (all_states, _) = $crate::slice::partition_dedup(&mut all_states);

                    for state in all_states {
                        writeln!(f, "  {state};")?;
                    }

                    $($(
                        $(
                            writeln!(
                                f,
                                "  {} -> {} [label = {:?}];",
                                stringify!($valid),
                                stringify!($target),
                                stringify!($event),
                            )?;
                        )*
                    )*)*

                    writeln!(f, "}}")?;
                    Ok(())
                }
            }

            Dot
        }
    }
}

pub use crate::__state_event__ as event;

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
    InvalidTransition { current: T, event: &'static str },
}

impl<T: fmt::Debug> fmt::Display for Error<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOp { current } => {
                write!(f, "state is already set to {current:?}")
            }
            Self::InvalidTransition { current, event } => {
                write!(f, "invalid event {event:?} for state {current:?}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl<T: fmt::Debug> std::error::Error for Error<T> {}

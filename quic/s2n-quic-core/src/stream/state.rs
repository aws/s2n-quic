// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::ensure;
use core::fmt;

pub type Result<T> = core::result::Result<(), Error<T>>;

macro_rules! transition {
    ($state:ident, $valid:pat => $target:expr) => {{
        ensure!(*$state != $target, Err(Error::NoOp { current: $target }));
        ensure!(
            matches!($state, $valid),
            Err(Error::InvalidTransition {
                current: $state.clone(),
                target: $target
            })
        );
        #[cfg(feature = "tracing")]
        {
            tracing::debug!(prev = ?$state, next = ?$target);
        }
        *$state = $target;
        Ok(())
    }};
}

macro_rules! is {
    ($($state:ident)|+, $function:ident) => {
        #[inline]
        pub fn $function(&self) -> bool {
            matches!(self, $(Self::$state)|*)
        }
    };
}

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

mod recv;
mod send;

pub use recv::Receiver;
pub use send::Sender;

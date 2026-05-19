// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(test, feature = "tracing"))]
pub use ::tracing::{debug, info, trace};

#[cfg(not(any(test, feature = "tracing")))]
#[macro_export]
macro_rules! __trace {
    ($($tt:tt)*) => { if false { ::tracing::trace!($($tt)*); } };
}

#[cfg(not(any(test, feature = "tracing")))]
#[macro_export]
macro_rules! __debug {
    ($($tt:tt)*) => { if false { ::tracing::debug!($($tt)*); } };
}

#[cfg(not(any(test, feature = "tracing")))]
#[macro_export]
macro_rules! __info {
    ($($tt:tt)*) => { if false { ::tracing::info!($($tt)*); } };
}

#[cfg(not(any(test, feature = "tracing")))]
pub use __debug as debug;
#[cfg(not(any(test, feature = "tracing")))]
pub use __info as info;
#[cfg(not(any(test, feature = "tracing")))]
pub use __trace as trace;

pub use ::tracing::{error, warn};

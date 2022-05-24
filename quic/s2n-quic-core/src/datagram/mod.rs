// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
pub mod default;
pub mod traits;
pub use traits::*;

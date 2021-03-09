// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod error_code;

pub use error_code::*;

/// Extension trait for errors that have an associated [`ApplicationErrorCode`]
pub trait ApplicationErrorExt {
    /// Returns the associated [`ApplicationErrorCode`], if any
    fn application_error_code(&self) -> Option<ApplicationErrorCode>;
}

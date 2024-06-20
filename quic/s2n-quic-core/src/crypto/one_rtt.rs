// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::{HeaderKey, Key};

/// Types for which are able to perform 1-RTT cryptography.
///
/// This trait ensures only 1-RTT-level keys
/// are used with Short packets. Any key misuses are
/// caught by the type system.
pub trait OneRttKey: Key {
    #[must_use]
    fn derive_next_key(&self) -> Self;
}

/// Types for which are able to perform 1-RTT header cryptography.
///
/// This trait ensures only 1-RTT-level header keys
/// are used with Short packets. Any key misuses are
/// caught by the type system.
pub trait OneRttHeaderKey: HeaderKey {}

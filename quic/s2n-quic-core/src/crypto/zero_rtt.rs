// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::{HeaderKey, Key};

/// Types for which are able to perform 0-RTT cryptography.
///
/// This marker trait ensures only 0-RTT-level keys
/// are used with ZeroRTT packets. Any key misuses are
/// caught by the type system.
pub trait ZeroRttKey: Key {}

/// Types for which are able to perform 0-RTT header cryptography.
///
/// This marker trait ensures only 0-RTT-level header keys
/// are used with ZeroRTT packets. Any key misuses are
/// caught by the type system.
pub trait ZeroRttHeaderKey: HeaderKey {}

/// ZeroRTT Secret tokens are always 32 bytes
pub type ZeroRttSecret = [u8; 32];

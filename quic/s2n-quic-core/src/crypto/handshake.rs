// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::{HeaderKey, Key};

/// Types for which are able to perform handshake cryptography.
///
/// This marker trait ensures only Handshake-level keys
/// are used with Handshake packets. Any key misuses are
/// caught by the type system.
pub trait HandshakeKey: Key {}

/// Types for which are able to perform handshake header cryptography.
///
/// This marker trait ensures only Handshake-level header keys
/// are used with Handshake packets. Any key misuses are
/// caught by the type system.
pub trait HandshakeHeaderKey: HeaderKey {}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::event::*;

/// Clients should implement the Publisher trait to customize which
/// events they want to emit from the library.
///
/// Since the default implementation is a noop, the rust compiler
/// is able to optimize away any allocations and code execution. This
/// results in zero-cost for any event we are not interested in consuming.
pub trait Publisher {
    fn on_version_information(&self, event: &VersionInformation) {
        let _ = event;
    }

    fn on_alpn_information(&self, event: &AlpnInformation) {
        let _ = event;
    }
}

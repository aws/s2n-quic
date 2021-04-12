// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::*;

/// Clients should implement the Subscriber trait to customize which
/// events they want to emit from the library.
///
/// Since the default implementation is a noop, the rust compiler
/// is able to optimize away any allocations and code execution. This
/// results in zero-cost for any event we are not interested in consuming.
pub trait Subscriber {
    fn on_version_information(&mut self, event: &VersionInformation) {
        let _ = event;
    }

    fn on_alpn_information(&mut self, event: &AlpnInformation) {
        let _ = event;
    }
}

impl<A, B> Subscriber for (A, B)
where
    A: Subscriber,
    B: Subscriber,
{
    fn on_version_information(&mut self, event: &VersionInformation) {
        self.0.on_version_information(event);
        self.1.on_version_information(event);
    }

    fn on_alpn_information(&mut self, event: &AlpnInformation) {
        self.0.on_alpn_information(event);
        self.1.on_alpn_information(event);
    }
}

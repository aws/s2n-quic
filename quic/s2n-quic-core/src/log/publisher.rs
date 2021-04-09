// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::log::event::*;

pub trait Publisher {
    fn on_version_information(&self, event: &VersionInformation) {
        let _ = event;
    }

    fn on_alpn_information(&self, event: &AlpnInformation) {
        let _ = event;
    }
}


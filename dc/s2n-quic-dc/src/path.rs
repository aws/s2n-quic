// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::path::{Handle, Tuple};

pub mod secret;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub trait Controller {
    type Handle: Handle;

    fn handle(&self) -> &Self::Handle;
}

impl Controller for Tuple {
    type Handle = Self;

    #[inline]
    fn handle(&self) -> &Self::Handle {
        self
    }
}

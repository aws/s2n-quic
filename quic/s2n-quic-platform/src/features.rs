// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use lazy_static::lazy_static;

mod gso;
pub use gso::Gso;

lazy_static! {
    static ref FEATURES: Features = Features::default();
}

pub fn get() -> &'static Features {
    &*FEATURES
}

#[derive(Debug, Default)]
pub struct Features {
    pub gso: Gso,
}

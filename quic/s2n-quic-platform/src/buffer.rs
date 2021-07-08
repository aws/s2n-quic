// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::{Deref, DerefMut};

mod vec;
// TODO support mmap buffers

pub use vec::*;

pub trait Buffer: Deref<Target = [u8]> + DerefMut {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn mtu(&self) -> usize;
}

pub mod default {
    pub use super::vec::VecBuffer as Buffer;
}

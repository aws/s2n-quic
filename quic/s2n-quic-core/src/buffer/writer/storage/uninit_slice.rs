// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{assume, buffer::writer::Storage};
use bytes::buf::UninitSlice;

impl Storage for &mut UninitSlice {
    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        unsafe {
            assume!(self.len() >= bytes.len());
        }
        self[..bytes.len()].copy_from_slice(bytes);
        let empty = UninitSlice::new(&mut []);
        let next = core::mem::replace(self, empty);
        *self = &mut next[bytes.len()..];
    }

    #[inline]
    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
    {
        ensure!(self.len() >= payload_len, Ok(false));

        f(&mut self[..payload_len])?;

        let empty = UninitSlice::new(&mut []);
        let next = core::mem::replace(self, empty);
        *self = &mut next[payload_len..];

        Ok(true)
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        self.len()
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod storage;

pub use storage::Storage;

/// A buffer capable of being written into by a reader
pub trait Writer {
    fn read_from<R>(&mut self, reader: &mut R) -> Result<(), super::Error<R::Error>>
    where
        R: super::Reader + ?Sized;
}

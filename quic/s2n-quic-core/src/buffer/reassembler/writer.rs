// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Reassembler;
use crate::buffer::{Error, Reader, Writer};

impl Writer for Reassembler {
    #[inline]
    fn read_from<R>(&mut self, reader: &mut R) -> Result<(), Error<R::Error>>
    where
        R: Reader + ?Sized,
    {
        self.write_reader(reader)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Reassembler;
use crate::{
    buffer::{duplex::Skip, Error},
    varint::VarInt,
};

impl Skip for Reassembler {
    #[inline]
    fn skip(&mut self, len: VarInt, final_offset: Option<VarInt>) -> Result<(), Error> {
        // write the final offset first, if possible
        if let Some(offset) = final_offset {
            self.write_at_fin(offset, &[])?;
        }

        // then skip the bytes
        (*self).skip(len)?;

        Ok(())
    }
}

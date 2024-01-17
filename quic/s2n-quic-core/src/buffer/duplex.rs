// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Error, Reader, Writer};
use crate::varint::VarInt;

mod split;

pub use split::Split;

/// A buffer that is capable of both reading and writing
pub trait Duplex: Reader + Writer {}

impl<T: Reader + Writer> Duplex for T {}

/// A buffer that is capable of both skipping a write and read with a given amount.
///
/// This can be used for scenarios where the buffer was written somewhere else but still needed to
/// be tracked.
pub trait Skip: Duplex {
    fn skip(&mut self, len: VarInt, final_offset: Option<VarInt>) -> Result<(), Error>;
}

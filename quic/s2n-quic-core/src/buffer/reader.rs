// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::varint::VarInt;

pub mod checked;
mod complete;
mod empty;
pub mod incremental;
mod limit;
pub mod storage;

pub use checked::Checked;
pub use complete::Complete;
pub use empty::Empty;
pub use incremental::Incremental;
pub use limit::Limit;
pub use storage::Storage;

/// A buffer that can be read with a tracked offset and final position.
pub trait Reader: Storage {
    /// Returns the currently read offset for the stream
    fn current_offset(&self) -> VarInt;

    /// Returns the final offset for the stream
    fn final_offset(&self) -> Option<VarInt>;

    /// Returns `true` if the reader has the final offset buffered
    #[inline]
    fn has_buffered_fin(&self) -> bool {
        self.final_offset().map_or(false, |fin| {
            let buffered_end = self
                .current_offset()
                .as_u64()
                .saturating_add(self.buffered_len() as u64);
            fin == buffered_end
        })
    }

    /// Returns `true` if the reader is finished producing data
    #[inline]
    fn is_consumed(&self) -> bool {
        self.final_offset()
            .map_or(false, |fin| fin == self.current_offset())
    }

    /// Limits the maximum offset that the caller can read from the reader
    #[inline]
    fn with_max_data(&mut self, max_data: VarInt) -> Limit<Self> {
        let max_buffered_len = max_data.saturating_sub(self.current_offset());
        let max_buffered_len = max_buffered_len.as_u64().min(self.buffered_len() as u64) as usize;
        self.with_read_limit(max_buffered_len)
    }

    /// Limits the maximum amount of data that the caller can read from the reader
    #[inline]
    fn with_read_limit(&mut self, max_buffered_len: usize) -> Limit<Self> {
        Limit::new(self, max_buffered_len)
    }

    /// Return an empty view onto the reader, with no change in current offset
    #[inline]
    fn with_empty_buffer(&self) -> Empty<Self> {
        Empty::new(self)
    }

    /// Enables checking the reader for correctness invariants
    #[inline]
    fn with_checks(&mut self) -> Checked<Self> {
        Checked::new(self)
    }
}

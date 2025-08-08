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

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// A buffer that can be read with a tracked offset and final position.
pub trait Reader: Storage {
    /// Returns the currently read offset for the stream
    fn current_offset(&self) -> VarInt;

    /// Returns the final offset for the stream
    fn final_offset(&self) -> Option<VarInt>;

    /// Returns `true` if the reader has the final offset buffered
    #[inline]
    fn has_buffered_fin(&self) -> bool {
        self.final_offset().is_some_and(|fin| {
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
            .is_some_and(|fin| fin == self.current_offset())
    }

    /// Skips the data in the reader until `offset` is reached, or the reader storage is exhausted.
    #[inline]
    fn skip_until(&mut self, offset: VarInt) -> Result<(), Self::Error> {
        ensure!(offset > self.current_offset(), Ok(()));

        while let Some(len) = offset.checked_sub(self.current_offset()) {
            let len = len.as_u64();

            // we don't need to skip anything if the difference is 0
            ensure!(len > 0, break);

            // clamp the len to usize
            let len = (usize::MAX as u64).min(len) as usize;
            let _chunk = self.read_chunk(len)?;

            ensure!(!self.buffer_is_empty(), break);
        }

        Ok(())
    }

    /// Limits the maximum offset that the caller can read from the reader
    #[inline]
    fn with_max_data(&mut self, max_data: VarInt) -> Limit<'_, Self> {
        let max_buffered_len = max_data.saturating_sub(self.current_offset());
        let max_buffered_len = max_buffered_len.as_u64().min(self.buffered_len() as u64) as usize;
        self.with_read_limit(max_buffered_len)
    }

    /// Limits the maximum amount of data that the caller can read from the reader
    #[inline]
    fn with_read_limit(&mut self, max_buffered_len: usize) -> Limit<'_, Self> {
        Limit::new(self, max_buffered_len)
    }

    /// Return an empty view onto the reader, with no change in current offset
    #[inline]
    fn with_empty_buffer(&self) -> Empty<'_, Self> {
        Empty::new(self)
    }

    /// Enables checking the reader for correctness invariants
    ///
    /// # Note
    ///
    /// `debug_assertions` must be enabled for these checks to be performed. Otherwise, the reader
    /// methods will simply be forwarded to `Self`.
    #[inline]
    fn with_checks(&mut self) -> Checked<'_, Self> {
        Checked::new(self)
    }
}

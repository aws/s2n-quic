// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod buf;
mod bytes;
mod chunk;
mod empty;
mod full_copy;
mod infallible;
mod io_slice;
mod slice;
mod tracked;

#[cfg(test)]
mod tests;

pub use buf::Buf;
pub use chunk::Chunk;
pub use empty::Empty;
pub use full_copy::FullCopy;
pub use infallible::Infallible;
pub use io_slice::IoSlice;
pub use tracked::Tracked;

pub trait Storage {
    type Error: 'static;

    /// Returns the length of the chunk
    fn buffered_len(&self) -> usize;

    /// Returns if the chunk is empty
    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.buffered_len() == 0
    }

    /// Reads the current contiguous chunk
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error>;

    /// Copies the reader into `dest`, with a trailing chunk of bytes.
    ///
    /// Implementations should either fill the `dest` completely or exhaust the buffered data.
    ///
    /// The storage also returns a `Chunk`, which can be used by the caller to defer
    /// copying the trailing chunk until later. The returned chunk must fit into the target
    /// destination. The caller must eventually copy the chunk into the destination, otherwise this
    /// data will be discarded.
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: crate::buffer::writer::Storage + ?Sized;

    /// Copies the reader into `dest`.
    ///
    /// Implementations should either fill the `dest` completely or exhaust the buffered data.
    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: crate::buffer::writer::Storage + ?Sized,
    {
        let mut chunk = self.partial_copy_into(dest)?;
        chunk.infallible_copy_into(dest);
        Ok(())
    }

    /// Forces the entire reader to be copied, even when calling `partial_copy_into`.
    ///
    /// The returned `Chunk` from `partial_copy_into` will always be empty.
    #[inline]
    fn full_copy(&mut self) -> FullCopy<'_, Self> {
        FullCopy::new(self)
    }

    /// Tracks the number of bytes read from the storage
    #[inline]
    fn track_read(&mut self) -> Tracked<'_, Self> {
        Tracked::new(self)
    }
}

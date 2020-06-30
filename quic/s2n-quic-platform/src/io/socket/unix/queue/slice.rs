#![allow(dead_code)] // only used in mmsg mode

use super::{Message, Segment};
use core::ops::{Deref, DerefMut};

/// A view of the currently enqueued messages for a given segment
#[derive(Debug)]
pub struct Slice<'a> {
    /// A slice of all of the messages in the buffer
    pub messages: &'a mut [Message],
    /// Reference to the primary segment
    pub primary: &'a mut Segment,
    /// Reference to the secondary segment
    pub secondary: &'a mut Segment,
}

impl<'a> Slice<'a> {
    /// Finishes the borrow of the `Slice` with a specified `count`
    ///
    /// Calling this method will move `count` messages from one segment
    /// to the other; e.g. `ready` to `pending`.
    pub fn finish(self, count: usize) {
        debug_assert!(
            count <= self.len(),
            "cannot finish more messages than available"
        );

        let capacity = self.primary.capacity;
        let prev_index = self.primary.index;

        // compute how many overflow messages were used
        let overflow_len = (prev_index + count).saturating_sub(capacity);

        // copy the overflowed message field lengths to the primary messages
        let (primary, secondary) = self.messages.split_at_mut(capacity);
        for (primary_msg, secondary_msg) in primary[..overflow_len]
            .iter_mut()
            .zip(secondary[..overflow_len].iter_mut())
        {
            primary_msg.copy_field_lengths_from(secondary_msg);
        }

        self.primary.move_into(self.secondary, count);
    }

    /// Preserves the messages in the current segment
    pub fn cancel(self) {
        // noop
    }
}

impl<'a> Deref for Slice<'a> {
    type Target = [Message];

    fn deref(&self) -> &Self::Target {
        let index = self.primary.index;
        let range = index..(index + self.primary.len);
        &self.messages[range]
    }
}

impl<'a> DerefMut for Slice<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let index = self.primary.index;
        let range = index..(index + self.primary.len);
        &mut self.messages[range]
    }
}

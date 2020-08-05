use super::{Behavior, Segment};
use crate::message;
use core::ops::{Deref, DerefMut};
use s2n_quic_core::io::{rx, tx};

/// A view of the currently enqueued messages for a given segment
#[derive(Debug)]
pub struct Slice<'a, Message, Behavior> {
    /// A slice of all of the messages in the buffer
    pub(crate) messages: &'a mut [Message],
    /// Reference to the primary segment
    pub(crate) primary: &'a mut Segment,
    /// Reference to the secondary segment
    pub(crate) secondary: &'a mut Segment,
    /// Reset the messages after use
    pub(crate) behavior: Behavior,
}

impl<'a, Message: message::Message, B: Behavior> Slice<'a, Message, B> {
    pub fn into_slice_mut(self) -> &'a mut [Message] {
        &mut self.messages[self.primary.range()]
    }

    /// Finishes the borrow of the `Slice` with a specified `count`
    ///
    /// Calling this method will move `count` messages from one segment
    /// to the other; e.g. `ready` to `pending`.
    pub fn finish(mut self, count: usize) {
        self.advance(count);
    }

    /// Advances the primary slice by the specified `count`
    fn advance(&mut self, count: usize) {
        debug_assert!(
            count <= self.len(),
            "cannot finish more messages than available"
        );

        let (start, end, overflow, capacity) = self.compute_behavior_arguments(count);

        let (primary, secondary) = self.messages.split_at_mut(capacity);

        self.behavior
            .advance(primary, secondary, start, end, overflow);
        self.primary.move_into(self.secondary, count);
    }

    /// Preserves the messages in the current segment
    pub fn cancel(self, count: usize) {
        let (start, end, overflow, capacity) = self.compute_behavior_arguments(count);

        let (primary, secondary) = self.messages.split_at_mut(capacity);

        self.behavior
            .cancel(primary, secondary, start, end, overflow);
    }

    #[inline]
    fn compute_behavior_arguments(&self, count: usize) -> (usize, usize, usize, usize) {
        let capacity = self.primary.capacity;
        let prev_index = self.primary.index;
        let new_index = prev_index + count;

        let start = prev_index;
        let end = new_index.min(capacity);
        let overflow = new_index.saturating_sub(capacity);

        (start, end, overflow, capacity)
    }
}

impl<'a, Message, R> Deref for Slice<'a, Message, R> {
    type Target = [Message];

    fn deref(&self) -> &Self::Target {
        &self.messages[self.primary.range()]
    }
}

impl<'a, Message, R> DerefMut for Slice<'a, Message, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.messages[self.primary.range()]
    }
}

impl<'a, Message: rx::Entry + message::Message, B: Behavior> rx::Queue for Slice<'a, Message, B> {
    type Entry = Message;

    fn as_slice_mut(&mut self) -> &mut [Message] {
        let range = self.primary.range();
        &mut self.messages[range]
    }

    fn finish(&mut self, count: usize) {
        self.advance(count)
    }
}

impl<'a, Message: tx::Entry + message::Message, B: Behavior> tx::Queue for Slice<'a, Message, B> {
    type Entry = Message;

    fn push<M: tx::Message>(&mut self, message: M) -> Result<usize, tx::Error> {
        let index = self
            .primary
            .index(&self.secondary)
            .ok_or_else(|| tx::Error::AtCapacity)?;

        self.messages[index].set(message)?;
        self.advance(1);

        Ok(index)
    }

    fn as_slice_mut(&mut self) -> &mut [Message] {
        &mut self.messages[self.secondary.range()]
    }

    fn capacity(&self) -> usize {
        self.primary.len
    }

    fn len(&self) -> usize {
        self.secondary.len
    }
}

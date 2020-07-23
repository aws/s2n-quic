mod cursor;
mod entry;
mod segment;
mod slice;

use core::fmt;
pub use cursor::Cursor;
pub use entry::Entry;
pub(crate) use segment::Segment;
pub use slice::Slice;

/// Structure for queueing network messages
///
/// Two segment queues are maintained: `ready` and `pending`. There is no predesignated meaning
/// or behavior and should be defined by the caller. When consuming a message from one segment
/// it will be moved to the other.
///
/// The payloads of the messages are backed by a parameterized
/// [`MessageBuffer`] to reduce allocations.
///
/// The queue uses a `Vec` of [`Message`]s double the length of the payload buffer.
/// The messages in the second half point to the same payloads as the first half, which
/// enables contiguous slices with arbitrary indexes. For example:
///
/// ```ignore
/// [ Message { payload: 0 }, Message { payload: 1 }, Message { payload: 2 }
///   Message { payload: 0 }, Message { payload: 1 }, Message { payload: 2 } ]
/// ```
///
/// Because the payloads at index `X` and `capacity + X` point to the same location in memory, the
/// messages are mostly interchangeable. When overflow messages are written to, some of the
/// fields need to be copied to the primary messages, which is handled by [`Cursor`] and [`Slice`].
///
/// To illustrate further, if the `ready` segment started at index 2 with a length of 2, the slice
/// returned would be:
///
/// ```ignore
/// [ Message { payload: 2 }, Message { payload: 0 } ]
/// ```
pub struct Queue<Ring: super::Ring> {
    ring: Ring,
    /// Segment of the `ready` messages
    ready: Segment,
    /// Segment of the `pending` messages
    pending: Segment,
}

impl<Ring: super::Ring> Queue<Ring> {
    /// Creates a new `MessageQueue` with a `MessageBuffer`
    pub fn new(ring: Ring) -> Self {
        let capacity = ring.len();
        let pending = Segment {
            index: 0,
            len: 0,
            capacity,
        };
        let ready = Segment {
            index: 0,
            len: capacity,
            capacity,
        };

        Self {
            ring,
            ready,
            pending,
        }
    }

    /// Returns the maximum size of a payload for any message
    pub fn mtu(&self) -> usize {
        self.ring.mtu()
    }

    /// Returns the number of slots in the buffer
    pub fn capacity(&self) -> usize {
        self.ring.len()
    }

    /// Tries to reserve a `ready` message to be moved to `pending`
    pub fn pop_ready(&mut self) -> Option<(Entry<Ring::Message>, Cursor)> {
        let capacity = self.capacity();
        let index = self.ready.index(&self.pending)?;

        let cursor = Cursor::new(&mut self.ready, &mut self.pending);
        let entry = Entry::new(self.ring.as_mut_slice(), index, capacity);

        Some((entry, cursor))
    }

    /// Tries to reserve a `pending` message to be moved to `ready`
    pub fn pop_pending(&mut self) -> Option<(Entry<Ring::Message>, Cursor)> {
        let capacity = self.capacity();
        let index = self.pending.index(&self.ready)?;

        let cursor = Cursor::new(&mut self.pending, &mut self.ready);
        let entry = Entry::new(self.ring.as_mut_slice(), index, capacity);

        Some((entry, cursor))
    }

    /// Returns the length of the `ready` message queue
    pub fn ready_len(&self) -> usize {
        self.ready.len
    }

    /// Returns the length of the `pending` message queue
    pub fn pending_len(&self) -> usize {
        self.pending.len
    }

    /// Returns a slice of all of the `ready` messages
    pub fn ready_mut(&mut self) -> Slice<Ring::Message> {
        Slice {
            messages: self.ring.as_mut_slice(),
            primary: &mut self.ready,
            secondary: &mut self.pending,
        }
    }

    /// Returns a slice of all of the `pending` messages
    pub fn pending_mut(&mut self) -> Slice<Ring::Message> {
        Slice {
            messages: self.ring.as_mut_slice(),
            primary: &mut self.pending,
            secondary: &mut self.ready,
        }
    }
}

impl<Ring: super::Ring> fmt::Debug for Queue<Ring> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Queue")
            .field("ready_index", &self.ready.index)
            .field("ready_len", &self.ready.len)
            .field("pending_index", &self.pending.index)
            .field("pending_len", &self.pending.len)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        buffer::VecBuffer,
        message::{simple::Ring, Message},
    };

    type Q = Queue<Ring<VecBuffer>>;

    fn new_queue(message_len: usize) -> Q {
        let payloads = VecBuffer::new(message_len, 1200);
        let ring = Ring::new(payloads);
        Queue::new(ring)
    }

    fn pop_ready_payload(queue: &mut Q, payload: &[u8]) {
        let (mut message, cursor) = queue.pop_ready().unwrap();
        unsafe {
            message.set_payload_len(payload.len());
        }
        message.payload_mut().copy_from_slice(payload);
        cursor.finish();
    }

    fn pop_pending_check(queue: &mut Q, payload: &[u8]) {
        let (mut message, cursor) = queue.pop_pending().unwrap();
        assert_eq!(message.payload_mut(), payload);
        cursor.finish();
    }

    #[test]
    fn pop_test() {
        let mut queue = new_queue(4);

        pop_ready_payload(&mut queue, &[0]);
        pop_ready_payload(&mut queue, &[1]);
        pop_ready_payload(&mut queue, &[2]);
        pop_ready_payload(&mut queue, &[3]);
        assert!(queue.pop_ready().is_none());

        pop_pending_check(&mut queue, &[0]);
        pop_pending_check(&mut queue, &[1]);
        pop_pending_check(&mut queue, &[2]);
        pop_pending_check(&mut queue, &[3]);
        assert!(queue.pop_pending().is_none());

        // make `cancel` works
        let (_, cursor) = queue.pop_ready().unwrap();
        cursor.cancel();
        assert_eq!(queue.ready_len(), 4);

        pop_ready_payload(&mut queue, &[4]);
        pop_pending_check(&mut queue, &[4]);
        assert!(queue.pop_pending().is_none());

        pop_ready_payload(&mut queue, &[5]);
        pop_ready_payload(&mut queue, &[6]);
        pop_pending_check(&mut queue, &[5]);
        pop_pending_check(&mut queue, &[6]);
        assert!(queue.pop_pending().is_none());
    }

    #[test]
    fn slice_test() {
        let mut queue = new_queue(4);

        pop_ready_payload(&mut queue, &[0]);
        pop_ready_payload(&mut queue, &[1]);
        pop_ready_payload(&mut queue, &[2]);
        pop_ready_payload(&mut queue, &[3]);
        assert!(queue.pop_ready().is_none());

        // there should be no remaining `ready` slots
        assert_eq!(queue.ready_mut().len(), 0);

        // make sure the payloads are correct
        let mut pending = queue.pending_mut();
        assert_eq!(pending.len(), 4);
        assert_eq!(pending[0].payload_mut(), &[0]);
        assert_eq!(pending[1].payload_mut(), &[1]);
        assert_eq!(pending[2].payload_mut(), &[2]);
        assert_eq!(pending[3].payload_mut(), &[3]);
        // partially consume the slice
        pending.finish(2);

        // test that cancelling works
        let mut ready = queue.ready_mut();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0].payload_mut(), &[0]);
        assert_eq!(ready[1].payload_mut(), &[1]);
        ready.cancel();

        // move 2 ready messages into pending
        let mut ready = queue.ready_mut();
        assert_eq!(ready.len(), 2);
        ready[0].payload_mut()[0] = 4;
        ready[1].payload_mut()[0] = 5;
        ready.finish(2);

        // make sure the payloads are correct
        let mut pending = queue.pending_mut();
        assert_eq!(pending.len(), 4);
        assert_eq!(pending[0].payload_mut(), &[2]);
        assert_eq!(pending[1].payload_mut(), &[3]);
        assert_eq!(pending[2].payload_mut(), &[4]);
        assert_eq!(pending[3].payload_mut(), &[5]);
        // consume all messages
        pending.finish(4);

        assert_eq!(queue.ready_mut().len(), 4);
        assert_eq!(queue.pending_mut().len(), 0);
    }
}

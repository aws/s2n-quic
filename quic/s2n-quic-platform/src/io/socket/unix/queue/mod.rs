mod cursor;
mod entry;
mod message;
mod msgname;
mod segment;
mod slice;

use crate::io::buffer::message::MessageBuffer;
use core::{
    fmt,
    mem::{size_of, zeroed},
};
use cursor::Cursor;
use entry::Entry;
use libc::{iovec, msghdr, sockaddr_in6};
use message::Message;
use segment::Segment;
use slice::Slice;

/// Structure for queueing network messages
///
/// Two segment queues are maintained: `ready` and `pending`. There is no predesignated meaning
/// or behavior and should be defined by the caller. When consuming a message from one segment
/// it will be moved to the other.
///
/// The payloads of the messages are backed by a parameterized
/// [`MessageBuffer`] to reduce allocations. Additionally, `iovec` and `msg_name` values
/// are pre-allocated internally.
///
/// The queue operates in either single message mode or
/// multi-message mode with the `mmsg` feature.
///
/// # Single message mode
/// The queue uses a `Vec` of [`Message`]s with equal length of the payload buffer
/// which can be popped individually. When finished, the message will move to the
/// opposing segment.
///
/// # Multi-message mode
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
pub struct MessageQueue<Buffer> {
    /// Buffer for the preallocated message payloads
    payloads: Buffer,
    /// Owning vector of all of the messages in the queue
    messages: Vec<Message>,
    /// Segment of the `ready` messages
    ready: Segment,
    /// Segment of the `pending` messages
    pending: Segment,
    /// The `iovecs` field is accessed through `messages`; this field
    /// exists to maintain ownership.
    #[allow(dead_code)]
    iovecs: Vec<iovec>,
    /// The `msg_names` field is accessed through `messages`; this field
    /// exists to maintain ownership.
    #[allow(dead_code)]
    msg_names: Vec<sockaddr_in6>,
}

/// The raw pointers contained inside the C structs do
/// not implement `Send` and rust needs to explicitly be told this
/// is ok.
unsafe impl<Buffer> Send for MessageQueue<Buffer> {}

impl<Buffer: MessageBuffer> MessageQueue<Buffer> {
    /// Creates a new `MessageQueue` with a `MessageBuffer`
    pub fn new(mut payloads: Buffer) -> Self {
        let capacity = payloads.len();
        let max_payload_size = payloads.slot_size();

        let mut iovecs = Vec::with_capacity(capacity);
        let mut msg_names = Vec::with_capacity(capacity);

        // double the capacity in both mmsg mode and test to allow for slice wrapping
        let mut messages = Vec::with_capacity(if cfg!(any(test, feature = "mmsg")) {
            capacity * 2
        } else {
            capacity
        });

        for index in 0..capacity {
            let mut msghdr = unsafe { zeroed::<msghdr>() };

            // Set the message iov to a single segment
            let mut iovec = unsafe { zeroed::<iovec>() };
            // assign the payload buffer to the single iovec
            iovec.iov_base = payloads[index].as_mut_ptr() as _;
            iovec.iov_len = max_payload_size;
            iovecs.push(iovec);

            // point the message to the current iovec
            msghdr.msg_iov = &mut iovecs[index];
            // There's only a single iovec for a given message
            msghdr.msg_iovlen = 1;

            // Set the msg_name (i.e. address) to a preallocated buffer
            msg_names.push(unsafe { zeroed::<sockaddr_in6>() });
            msghdr.msg_name = &msg_names[index] as *const _ as _;
            msghdr.msg_namelen = size_of::<sockaddr_in6>() as _;

            // TODO support ecn
            // msghdr.msg_control = cmsg.as_mut_ptr() as _;
            // msghdr.msg_controllen = CMSG_LEN as _;

            messages.push(Message::new(msghdr));
        }

        #[cfg(any(test, feature = "mmsg"))]
        for index in 0..capacity {
            // Clone all of the messages in the queue to implement a ring buffer that
            // can be viewed in a single continuous slice
            let message = unsafe { messages[index].create_multi_owner() };
            messages.push(message);
        }

        Self {
            payloads,
            iovecs,
            msg_names,
            messages,
            pending: Segment {
                index: 0,
                len: 0,
                capacity,
            },
            ready: Segment {
                index: 0,
                len: capacity,
                capacity,
            },
        }
    }

    /// Returns the maximum size of a payload for any message
    pub fn max_payload_size(&self) -> usize {
        self.payloads.slot_size()
    }

    /// Returns the number of slots in the buffer
    pub fn capacity(&self) -> usize {
        self.payloads.len()
    }

    /// Tries to reserve a `ready` message to be moved to `pending`
    pub fn pop_ready(&mut self) -> Option<(Entry, Cursor)> {
        let capacity = self.capacity();
        let index = self.ready.index(&self.pending)?;

        let cursor = Cursor::new(&mut self.ready, &mut self.pending);
        let entry = Entry::new(&mut self.messages, index, capacity);

        Some((entry, cursor))
    }

    /// Tries to reserve a `pending` message to be moved to `ready`
    pub fn pop_pending(&mut self) -> Option<(Entry, Cursor)> {
        let capacity = self.capacity();
        let index = self.pending.index(&self.ready)?;

        let cursor = Cursor::new(&mut self.pending, &mut self.ready);
        let entry = Entry::new(&mut self.messages, index, capacity);

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
    #[allow(dead_code)] // only used in mmsg mode
    pub fn ready_mut(&mut self) -> Slice {
        Slice {
            messages: &mut self.messages,
            primary: &mut self.ready,
            secondary: &mut self.pending,
        }
    }

    /// Returns a slice of all of the `pending` messages
    #[allow(dead_code)] // only used in mmsg mode
    pub fn pending_mut(&mut self) -> Slice {
        Slice {
            messages: &mut self.messages,
            primary: &mut self.pending,
            secondary: &mut self.ready,
        }
    }
}

impl<Buffer> fmt::Debug for MessageQueue<Buffer> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MessageQueue")
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
    use crate::io::buffer::VecBuffer;

    fn new_queue(slot_count: usize) -> MessageQueue<VecBuffer> {
        MessageQueue::new(VecBuffer::with_slot_count(slot_count, 1200))
    }

    fn pop_ready_payload(queue: &mut MessageQueue<VecBuffer>, payload: &[u8]) {
        let (mut message, cursor) = queue.pop_ready().unwrap();
        message.set_payload_len(payload.len());
        message.payload_mut().copy_from_slice(payload);
        cursor.finish();
    }

    fn pop_pending_check(queue: &mut MessageQueue<VecBuffer>, payload: &[u8]) {
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

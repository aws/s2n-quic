// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod behavior;
mod segment;
mod slice;

pub use behavior::Behavior;
pub(crate) use segment::Segment;
pub use slice::Slice;

pub type Free<'a, M> = Slice<'a, M, behavior::Free>;
pub type Occupied<'a, M> = Slice<'a, M, behavior::Occupied>;
pub type OccupiedWipe<'a, M> = Slice<'a, M, behavior::OccupiedWipe>;

use crate::message;
use core::fmt;

/// Structure for queueing network messages
///
/// Two segment queues are maintained: `free` and `occupied`. When consuming a message from one segment
/// it will be moved to the other.
///
/// The payloads of the messages are backed by a parameterized
/// [`message::Ring`] to reduce allocations.
///
/// The queue uses a [`Vec`] of [`message::Message`]s double the length of the payload buffer.
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
/// fields need to be copied to the primary messages, which is handled by [`Slice`].
///
/// To illustrate further, if the `free` segment started at index 2 with a length of 2, the slice
/// returned would be:
///
/// ```ignore
/// [ Message { payload: 2 }, Message { payload: 0 } ]
/// ```
pub struct Queue<Ring: message::Ring> {
    ring: Ring,
    /// Segment of the `occupied` messages
    occupied: Segment,
    /// Segment of the `free` messages
    free: Segment,
}

impl<Ring: message::Ring + Default> Default for Queue<Ring> {
    fn default() -> Self {
        Self::new(Ring::default())
    }
}

impl<Ring: message::Ring> Queue<Ring> {
    /// Creates a new `MessageQueue` with a `MessageBuffer`
    pub fn new(ring: Ring) -> Self {
        let capacity = ring.len();
        let occupied = Segment {
            index: 0,
            len: 0,
            capacity,
        };
        let free = Segment {
            index: 0,
            len: capacity,
            capacity,
        };

        Self {
            ring,
            occupied,
            free,
        }
    }

    /// Returns the maximum size of a payload for any message
    pub fn mtu(&self) -> usize {
        self.ring.mtu()
    }

    pub fn max_gso(&self) -> usize {
        self.ring.max_gso()
    }

    pub fn disable_gso(&mut self) {
        self.ring.disable_gso()
    }

    /// Returns the number of slots in the buffer
    pub fn capacity(&self) -> usize {
        self.ring.len()
    }

    /// Returns the length of the `free` message queue
    pub fn free_len(&self) -> usize {
        self.free.len
    }

    /// Returns the length of the `occupied` message queue
    pub fn occupied_len(&self) -> usize {
        self.occupied.len
    }

    /// Returns a slice of all of the `free` messages
    pub fn free_mut(&mut self) -> Free<Ring::Message> {
        let mtu = self.mtu();
        let max_gso = self.max_gso();
        Slice {
            messages: self.ring.as_mut_slice(),
            primary: &mut self.free,
            secondary: &mut self.occupied,
            behavior: behavior::Free { mtu },
            max_gso,
            gso_segment: None,
        }
    }

    /// Returns a slice of all of the `occupied` messages
    pub fn occupied_mut(&mut self) -> Occupied<Ring::Message> {
        let mtu = self.mtu();
        let max_gso = self.max_gso();
        Slice {
            messages: self.ring.as_mut_slice(),
            primary: &mut self.occupied,
            secondary: &mut self.free,
            behavior: behavior::Occupied { mtu },
            max_gso,
            gso_segment: None,
        }
    }

    /// Returns a slice of all of the `occupied` messages
    ///
    /// The messages will be wiped on release.
    pub fn occupied_wipe_mut(&mut self) -> OccupiedWipe<Ring::Message> {
        let mtu = self.mtu();
        let max_gso = self.max_gso();
        Slice {
            messages: self.ring.as_mut_slice(),
            primary: &mut self.occupied,
            secondary: &mut self.free,
            behavior: behavior::OccupiedWipe { mtu },
            max_gso,
            gso_segment: None,
        }
    }
}

impl<Ring: message::Ring> fmt::Debug for Queue<Ring> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Queue")
            .field("free_index", &self.free.index)
            .field("free_len", &self.free.len)
            .field("occupied_index", &self.occupied.index)
            .field("occupied_len", &self.occupied.len)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{buffer::VecBuffer, message::Message};
    use bolero::{check, generator::*};
    use s2n_quic_core::inet;
    use std::collections::VecDeque;

    const MTU: usize = 1200;

    fn set<M: Message>(message: &mut M, value: u8, len: usize) {
        assert_eq!(
            message.payload_len(),
            MTU,
            "payload len should be reset for free messages"
        );
        unsafe {
            message.set_payload_len(len);
        }
        for b in message.payload_mut().iter_mut() {
            *b = value;
        }
    }

    fn gen_address() -> impl ValueGenerator<Output = inet::SocketAddress> {
        #[cfg(feature = "ipv6")]
        let generator = gen();

        #[cfg(not(feature = "ipv6"))]
        let generator = gen::<inet::SocketAddressV4>().map(|addr| addr.into());

        generator
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Operation {
        Push {
            /// Number of payloads to push
            #[generator(0..20)]
            count: usize,

            /// Length of the payload to be pushed
            #[generator(1..32)]
            len: usize,

            #[generator(gen_address())]
            address: inet::SocketAddress,

            /// true if the operation is successful
            success: bool,
        },
        Pop {
            /// Number of payloads to pop
            #[generator(0..20)]
            count: usize,

            /// true if the operation is successful
            success: bool,
        },
    }

    fn check<R: message::Ring>(mut queue: Queue<R>, capacity: usize, ops: &[Operation]) {
        let mut oracle = VecDeque::new();
        let mut value = 0u8;
        for op in ops {
            match *op {
                Operation::Push {
                    count,
                    len,
                    address,
                    success,
                } => {
                    let mut free = queue.free_mut();
                    let count = count.min(free.len());
                    let mut payload = value;

                    // push messages onto the queue and the oracle
                    for message in &mut free[..count] {
                        set(message, payload, len);

                        message.set_remote_address(&address);
                        oracle.push_back((address, len, payload));
                        payload = payload.wrapping_add(1);
                    }

                    // if successful, finish the slice, otherwise cancel
                    if success {
                        value = payload;
                        free.finish(count);
                    } else {
                        oracle.drain((oracle.len() - count)..);
                        free.cancel(count);
                    }
                }
                Operation::Pop { count, success } => {
                    let occupied = queue.occupied_mut();
                    let count = count.min(occupied.len());

                    // if successful, finish the slice, otherwise cancel
                    if success {
                        occupied.finish(count);
                        oracle.drain(..count);
                    } else {
                        occupied.cancel(count);
                    }
                }
            }

            assert_eq!(capacity, queue.capacity());
            assert_eq!(capacity, queue.occupied_len() + queue.free_len());

            // assert the queue matches the oracle
            let occupied = queue.occupied_mut();
            assert_eq!(oracle.len(), occupied.len());

            for (message, (address, len, value)) in occupied.iter().zip(oracle.iter()) {
                let address = *address;

                #[cfg(all(target_os = "macos", feature = "ipv6"))]
                let address = address.to_ipv6_mapped().into();

                assert_eq!(message.remote_address(), Some(address));
                assert_eq!(message.payload_len(), *len);
                assert!(message.payload().iter().all(|v| v == value));
            }
        }
    }

    macro_rules! differential_test {
        ($name:ident, $ring:path) => {
            /// A VecDeque is used to assert the behavior matches the Queue
            #[test]
            fn $name() {
                check!()
                    .with_generator((0usize..16, gen::<Vec<Operation>>()))
                    .for_each(|(capacity, ops)| {
                        use $ring;
                        let payloads = VecBuffer::new(*capacity, MTU);
                        // limit GSO segments as this harness assumes no GSO
                        let max_gso = 1;
                        let ring = Ring::new(payloads, max_gso);
                        let queue = Queue::new(ring);
                        assert_eq!(queue.mtu(), MTU);
                        check(queue, *capacity, ops);
                    });
            }
        };
    }

    differential_test!(simple_differential_test, message::simple::Ring);

    #[cfg(s2n_quic_platform_socket_msg)]
    differential_test!(msg_differential_test, message::msg::Ring);

    #[cfg(s2n_quic_platform_socket_mmsg)]
    differential_test!(mmsg_differential_test, message::mmsg::Ring);
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::if_xdp::UmemDescriptor;

/// Trait to define how descriptors are assigned to TX workers
///
/// As the Completion ring is global for the entire socket, it is up to the application to decide
/// which TX queues get which descriptors. This trait takes in a descriptor and decides if it
/// pertains to a worker index or not.
pub trait Assign: Unpin {
    fn assign(&self, desc: UmemDescriptor, idx: u64) -> bool;
}

impl Assign for () {
    #[inline]
    fn assign(&self, _desc: UmemDescriptor, idx: u64) -> bool {
        debug_assert_eq!(
            idx, 0,
            "assignment mode should only be used for single queue workflows"
        );

        // only assign descriptors to the first worker
        idx == 0
    }
}

/// Assignment strategy that is generic over framing and index alignment
pub struct AssignGeneric<F: FrameToIndex, I: IndexToQueue> {
    pub frame: F,
    pub index: I,
}

impl<F: FrameToIndex, I: IndexToQueue> Assign for AssignGeneric<F, I> {
    #[inline]
    fn assign(&self, desc: UmemDescriptor, idx: u64) -> bool {
        let v = self.frame.frame_to_index(desc);
        let v = self.index.index_to_queue(v);
        v == idx
    }
}

/// Converts a frame address into a frame index
pub trait FrameToIndex: Unpin {
    fn frame_to_index(&self, desc: UmemDescriptor) -> u64;
}

pub struct AlignedFrame {
    shift: u32,
}

impl AlignedFrame {
    pub fn new(frame_size: u32) -> Self {
        debug_assert!(frame_size.is_power_of_two());

        let shift = frame_size.trailing_zeros();

        debug_assert_eq!(
            frame_size,
            2u32.pow(shift),
            "computing the square root of a power of two is counting the trailing zeros"
        );

        Self { shift }
    }
}

impl FrameToIndex for AlignedFrame {
    #[inline]
    fn frame_to_index(&self, desc: UmemDescriptor) -> u64 {
        desc.address >> self.shift
    }
}

pub struct UnalignedFrame {
    frame_size: u64,
}

impl UnalignedFrame {
    pub fn new(frame_size: u32) -> Self {
        let frame_size = frame_size as u64;
        debug_assert!(!frame_size.is_power_of_two());
        Self { frame_size }
    }
}

impl FrameToIndex for UnalignedFrame {
    #[inline]
    fn frame_to_index(&self, desc: UmemDescriptor) -> u64 {
        desc.address / self.frame_size
    }
}

/// Converts a frame index into a queue index
pub trait IndexToQueue: Unpin {
    fn index_to_queue(&self, index: u64) -> u64;
}

pub struct AlignedQueue {
    mask: u64,
}

impl AlignedQueue {
    pub fn new(queues: usize) -> Self {
        let queues = queues as u64;
        debug_assert!(queues.is_power_of_two());
        let mask = queues - 1;
        Self { mask }
    }
}

impl IndexToQueue for AlignedQueue {
    #[inline]
    fn index_to_queue(&self, index: u64) -> u64 {
        index & self.mask
    }
}

pub struct UnalignedQueue {
    queues: u64,
}

impl UnalignedQueue {
    pub fn new(queues: usize) -> Self {
        let queues = queues as u64;
        debug_assert!(!queues.is_power_of_two());
        Self { queues }
    }
}

impl IndexToQueue for UnalignedQueue {
    #[inline]
    fn index_to_queue(&self, index: u64) -> u64 {
        index % self.queues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::if_xdp::UmemDescriptor;
    use bolero::check;

    #[cfg(not(kani))]
    fn test_generic<F: FrameToIndex, I: IndexToQueue>(
        frame_size: u32,
        frame: F,
        queues: usize,
        index: I,
    ) {
        let assigner = AssignGeneric { frame, index };

        let indexes = 0u64..100;
        let mut expected_queue = (0..queues as u64).cycle();

        for desc in indexes.map(|idx| UmemDescriptor {
            address: idx * frame_size as u64,
        }) {
            let expected_queue = expected_queue.next().unwrap();
            for queue in 0..queues as u64 {
                let is_expected = queue == expected_queue;

                for offset in [0, 1, 2, (frame_size - 1) as _] {
                    let mut desc = desc;
                    desc.address += offset;
                    let was_assigned = assigner.assign(desc, queue as _);
                    assert_eq!(
                        is_expected, was_assigned,
                        "desc: {desc:?}, expected_queue: {expected_queue}, queue: {queue}"
                    );
                }
            }
        }
    }

    #[cfg(kani)]
    fn test_generic<F: FrameToIndex, I: IndexToQueue>(
        frame_size: u32,
        frame: F,
        queues: usize,
        index: I,
    ) {
        let assigner = AssignGeneric { frame, index };

        let address: u64 = kani::any();
        let expected_queue = (address / frame_size as u64) % queues as u64;

        let queue: u64 = kani::any();
        kani::assume(queue <= queues as u64);

        let desc = UmemDescriptor { address };

        let is_expected = queue == expected_queue;

        let was_assigned = assigner.assign(desc, queue);
        assert_eq!(is_expected, was_assigned,);
    }

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(4), kani::solver(kissat))]
    fn assignment_test() {
        // The kani proof takes about 1m with the current parameters. Increasing any of these
        // numbers causes it to take much longer - but I didn't take the time to find out _how_
        // long. Either way, the current bounds should be sufficient to show that the math works.
        let frames = if cfg!(kani) { 4u32..=6 } else { 4u32..=100_000 };
        let queues = if cfg!(kani) { 1usize..=4 } else { 1usize..=128 };

        check!()
            .with_generator((frames, queues))
            .cloned()
            .for_each(|(frame_size, queues)| {
                match (frame_size.is_power_of_two(), queues.is_power_of_two()) {
                    (true, true) => test_generic(
                        frame_size,
                        AlignedFrame::new(frame_size),
                        queues,
                        AlignedQueue::new(queues),
                    ),
                    (true, false) => test_generic(
                        frame_size,
                        AlignedFrame::new(frame_size),
                        queues,
                        UnalignedQueue::new(queues),
                    ),
                    (false, true) => test_generic(
                        frame_size,
                        UnalignedFrame::new(frame_size),
                        queues,
                        AlignedQueue::new(queues),
                    ),
                    (false, false) => test_generic(
                        frame_size,
                        UnalignedFrame::new(frame_size),
                        queues,
                        UnalignedQueue::new(queues),
                    ),
                }
            });
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::StreamId;
#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

/// An Iterator over Stream Ids of a particular type.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
pub struct StreamIter {
    start_stream_id: StreamId,
    max_stream_id: StreamId,
    finished: bool,
}

impl StreamIter {
    #[inline]
    pub fn new(start_stream_id: StreamId, max_stream_id: StreamId) -> Self {
        debug_assert_eq!(start_stream_id.stream_type(), max_stream_id.stream_type());
        debug_assert_eq!(start_stream_id.initiator(), max_stream_id.initiator());
        debug_assert!(start_stream_id <= max_stream_id);

        Self {
            start_stream_id,
            max_stream_id,
            finished: false,
        }
    }

    #[inline]
    pub fn max_stream_id(self) -> StreamId {
        self.max_stream_id
    }
}

impl Iterator for StreamIter {
    type Item = StreamId;

    fn next(&mut self) -> Option<Self::Item> {
        // short circuit when finished
        if self.finished {
            return None;
        }

        match self.start_stream_id.cmp(&self.max_stream_id) {
            core::cmp::Ordering::Less => {
                let ret = self.start_stream_id;
                // The Stream ID can be expected to be valid, since `max_stream_id`
                // is a valid `StreamId` and all IDs we iterate over are lower.
                self.start_stream_id = self
                    .start_stream_id
                    .next_of_type()
                    .expect("Expect a valid Stream ID");
                Some(ret)
            }
            core::cmp::Ordering::Equal => {
                // Avoid incrementing beyond `max_stream_id` and mark finished to
                // to avoid returning max value again
                self.finished = true;
                Some(self.start_stream_id)
            }
            core::cmp::Ordering::Greater => {
                debug_assert!(false, "The `new` method should verify valid ranges");

                // finished
                self.finished = true;
                None
            }
        }
    }
}

#[cfg(test)]
mod fuzz_target {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
    #[cfg_attr(kani, kani::proof, kani::unwind(1), kani::solver(kissat))]
    fn fuzz_builder() {
        bolero::check!()
            .with_type::<(StreamId, StreamId)>()
            .cloned()
            .for_each(|(min, max)| {
                // enforce min <= max
                if min > max {
                    return;
                }
                // enforce same initiator type
                if min.initiator() != max.initiator() {
                    return;
                }
                // enforce same stream type
                if min.stream_type() != max.stream_type() {
                    return;
                }

                // All other combinations of min/max StreamId should be valid
                StreamIter::new(min, max);
            });
    }
}

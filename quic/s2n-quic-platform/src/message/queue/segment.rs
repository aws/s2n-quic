// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::Range;

/// Maintains information for a single queue segment of messages.
///
/// A segment is a subsection of the total capacity of messages
/// in the queue. Each queue has 2 segments: `ready` and `pending`.
/// When a message is consumed in one segment, it is moved to the
/// other.
#[derive(Debug)]
pub struct Segment {
    pub index: usize,
    pub len: usize,
    pub capacity: usize,
}

impl Segment {
    /// Returns the current starting index in the message buffer for the given segment
    pub fn index(&self, other: &Self) -> Option<usize> {
        // handle the case where the segment has no remaining capacity
        if self.index == other.index && self.len == 0 {
            None
        } else {
            Some(self.index)
        }
    }

    /// Returns the range of messages for the given segment
    pub fn range(&self) -> Range<usize> {
        let index = self.index;
        index..(index + self.len)
    }

    /// Moves `count` number of messages from one segment to the other
    pub fn move_into(&mut self, other: &mut Self, count: usize) {
        debug_assert!(
            count <= self.len,
            "cannot move more messages than {}, tried to move {}",
            self.len,
            count
        );

        // Increment the index by count and wrap by capacity
        let index = self.index + count;
        self.index = if let Some(index) = index.checked_sub(self.capacity) {
            index
        } else {
            index
        };

        // take the len from primary and move it to secondary
        self.len -= count;
        other.len += count;
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, ops};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

#[derive(Clone)]
pub struct Segment(Arc<Vec<u8>>);

impl Segment {
    #[inline]
    pub fn make_mut(&mut self) -> &mut Vec<u8> {
        Arc::make_mut(&mut self.0)
    }
}

impl ops::Deref for Segment {
    type Target = Vec<u8>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Vec<u8>> for Segment {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self(value.into())
    }
}

impl fmt::Debug for Segment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buffer::Segment")
            .field("len", &self.0.len())
            .finish()
    }
}

#[derive(Debug)]
pub struct Allocator {
    free: ArrayQueue<Segment>,
}

impl Default for Allocator {
    #[inline]
    fn default() -> Self {
        Self {
            // TODO is this an OK default?
            free: ArrayQueue::new(32),
        }
    }
}

impl Allocator {
    #[inline]
    pub fn alloc(&self, capacity: usize) -> Segment {
        self.free
            .pop()
            .filter(|buffer| buffer.capacity() >= capacity)
            .unwrap_or_else(|| Vec::with_capacity(capacity).into())
    }

    #[inline]
    pub fn free(&self, mut segment: Segment) {
        if let Some(buffer) = Arc::get_mut(&mut segment.0) {
            // clear the buffer if it has bytes in it
            buffer.clear();

            // try to push to the free queue
            let _ = self.free.push(segment);
        }
    }
}

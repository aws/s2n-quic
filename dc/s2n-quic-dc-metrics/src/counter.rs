// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::rseq::{Absorb, Channels};
use std::sync::Arc;

/// A `Counter` can only be incremented.
///
/// Counters are cleared at each reporting interval, such that the sum and avg
/// statistics are correctly computed in iGraph.
#[derive(Clone)]
pub struct Counter {
    channels: Arc<Channels<SharedCounter>>,
    counter: u32,
}

impl std::fmt::Debug for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Counter")
            .field("counter", &self.counter)
            .finish()
    }
}

#[derive(Default, Debug)]
pub(crate) struct SharedCounter {
    pub(crate) value: u64,
}

impl Absorb for SharedCounter {
    fn handle(slots: &mut [Self], events: &mut [u64]) {
        let (chunks, tail) = events.as_chunks::<8>();
        for chunk in chunks {
            for event in chunk {
                let idx = (*event >> 32) as usize;
                slots[idx].value += *event as u32 as u64;
            }
        }

        for event in tail {
            let idx = (*event >> 32) as usize;
            slots[idx].value += *event as u32 as u64;
        }
    }
}

impl Counter {
    pub(crate) fn new(channels: Arc<Channels<SharedCounter>>) -> Counter {
        Counter {
            counter: channels.allocate(),
            channels,
        }
    }

    #[track_caller]
    pub fn increment(&self, count: u64) {
        assert!(count <= u32::MAX as u64);
        self.channels
            .send_event(((self.counter as u64) << 32) | count);
    }

    pub(crate) fn take_current(&self) -> Option<String> {
        let value = self.channels.get_mut(self.counter, std::mem::take);
        Some(format!("{}", value.value))
    }
}

#[test]
fn basic() {
    let registry = crate::Registry::new();
    let a = registry.register_counter(String::from("a"), None);
    let b = registry.register_counter(String::from("b"), None);

    a.increment(5);
    b.increment(9);

    assert_eq!(registry.take_current_metrics_line(), "a=5,b=9");
}

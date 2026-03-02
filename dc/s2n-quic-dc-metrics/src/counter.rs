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

    pub fn increment(&self, count: u64) {
        // If we get a particularly large count, directly serialize it into the underlying
        // aggregate. We expect this to be rare, so acquiring the lock across all counters should
        // be relatively cheap.
        //
        // It's possible to shift the exact threshold if we wanted to (e.g., by reducing the
        // counter index bits, or having a more complicated serialization scheme), but we don't
        // expect this to matter much in practice. If the event recorded is this large there's
        // probably a good deal of compute needed to produce it in the first place.
        if count > (u32::MAX as u64) {
            self.channels.lock_aggregate()[self.counter as usize].value += count;
            return;
        }
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

#[test]
fn check_u64_max() {
    let registry = crate::Registry::new();
    let a = registry.register_counter(String::from("a"), None);
    let b = registry.register_counter(String::from("b"), None);

    a.increment(u64::MAX);
    b.increment(u32::MAX as u64);

    assert_eq!(
        registry.take_current_metrics_line(),
        format!("a={},b={}", u64::MAX, u32::MAX)
    );
}

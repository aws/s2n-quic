// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Additional tests for the hierarchical timing wheel

use super::*;
use crate::{
    intrusive::{EntryAdapter, Queue},
    socket::channel::Receiver as _,
    time::{
        precision::Clock as _,
        testing::{Clock, Timer as ClockTimer},
    },
};
use core::pin::pin;
use s2n_quic_core::task::waker;
use std::{collections::BTreeMap, time::Duration};

type TestWheel<'a> = Wheel<EntryAdapter<TestEntry>, ClockTimer, &'a TestChannel, 1>;

// ── Test utilities ─────────────────────────────────────────────────────

/// Poll a future once and return the result if Ready, or None if Pending
fn poll_once<F: core::future::Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    let waker = waker::noop();
    let mut cx = core::task::Context::from_waker(&waker);

    match future.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(output) => Some(output),
        core::task::Poll::Pending => None,
    }
}

// Mock entry type for testing
struct TestEntry {
    meta: u16,
    transmission_time: Option<precision::Timestamp>,
}

impl SingleTimer for TestEntry {
    fn target_time(&self) -> Option<precision::Timestamp> {
        self.transmission_time
    }

    fn set_target_time(&mut self, time: precision::Timestamp) {
        self.transmission_time = Some(time);
    }
}

// Simple channel for testing
struct TestChannel {
    queue: std::sync::Mutex<std::collections::VecDeque<Queue<TestEntry>>>,
    closed: std::sync::atomic::AtomicBool,
}

impl TestChannel {
    fn new() -> Self {
        Self {
            queue: std::sync::Mutex::new(std::collections::VecDeque::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn send_batch(&self, batch: Queue<TestEntry>) {
        self.queue.lock().unwrap().push_back(batch);
    }
}

impl channel::Receiver<Queue<TestEntry>> for &TestChannel {
    fn poll_recv(
        &mut self,
        _cx: &mut task::Context<'_>,
        _budget: &mut channel::Budget,
    ) -> task::Poll<Option<Queue<TestEntry>>> {
        if let Some(batch) = self.queue.lock().unwrap().pop_front() {
            return task::Poll::Ready(Some(batch));
        }

        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return task::Poll::Ready(None);
        }

        task::Poll::Pending
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn test_immediate_transmission() {
    let clock = Clock::new(Duration::from_micros(1000));
    let channel = TestChannel::new();
    let mut wheel: TestWheel = Wheel::new(&channel, clock.timer());

    // Entry with None timestamp should bypass wheel and be immediately available
    let mut batch = Queue::new();
    batch.push_back(
        TestEntry {
            meta: 42,
            transmission_time: None,
        }
        .into(),
    );
    channel.send_batch(batch);

    let mut queue = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }))
    .unwrap()
    .unwrap();

    assert_eq!(queue.pop_front().unwrap().meta, 42);
    assert!(queue.is_empty());
}

#[test]
fn test_len_tracking() {
    let clock = Clock::new(Duration::from_micros(1000));
    let channel = TestChannel::new();
    let mut wheel: TestWheel = Wheel::new(&channel, clock.timer());

    assert_eq!(wheel.len, 0);

    // Send 5 entries with future timestamps (should stay in wheel)
    let future_time = clock.get_time() + Duration::from_micros(10);
    let mut batch = Queue::new();
    for i in 0..5 {
        batch.push_back(
            TestEntry {
                meta: 100 + i,
                transmission_time: Some(future_time),
            }
            .into(),
        );
    }
    channel.send_batch(batch);

    // Poll to insert them into wheel - should insert and check if any are ready
    let result = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }));
    assert!(
        result.is_none(),
        "Entries should not be ready yet (they're in the future)"
    );
    assert_eq!(wheel.len, 5, "Len should be 5 after inserting");

    // Advance timer to future time and drain them
    clock.set(future_time);
    let mut queue = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }))
    .unwrap()
    .unwrap();

    while queue.pop_front().is_some() {}
    assert_eq!(wheel.len, 0, "Len should be 0 after draining");
}

#[test]
fn test_cascade() {
    // With GRANULARITY_US=1 and 256 slots/level, we need >256 ticks to trigger cascade
    let clock = Clock::new(Duration::from_micros(1000));
    let channel = TestChannel::new();
    let mut wheel: TestWheel = Wheel::new(&channel, clock.timer());

    // Insert entry 300 µs in future (beyond level-0's 256-slot range)
    let future_time = clock.get_time() + Duration::from_micros(300);
    let mut batch = Queue::new();
    batch.push_back(
        TestEntry {
            meta: 999,
            transmission_time: Some(future_time),
        }
        .into(),
    );
    channel.send_batch(batch);

    // Poll to insert
    let _ = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }));

    // Advance timer to 256 ticks - should still be pending (in level 1)
    clock.advance(Duration::from_micros(256));
    let result = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }));
    assert!(result.is_none(), "Entry should not be ready yet");

    // Advance to target time - should get the entry after cascade
    clock.set(future_time);
    let mut queue = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }))
    .unwrap()
    .unwrap();

    assert_eq!(queue.pop_front().unwrap().meta, 999);
}

#[test]
fn test_ordering() {
    let clock = Clock::new(Duration::from_micros(1000));
    let channel = TestChannel::new();
    let mut wheel: TestWheel = Wheel::new(&channel, clock.timer());

    // Insert entries in reverse order - wheel should reorder by time
    // Start at 1 to avoid entries at current time (which are immediately ready)
    let mut batch = Queue::new();
    for i in (1..11u16).rev() {
        batch.push_back(
            TestEntry {
                meta: i,
                transmission_time: Some(clock.get_time() + Duration::from_micros(i as u64)),
            }
            .into(),
        );
    }
    channel.send_batch(batch);

    // Poll to insert
    let _ = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }));

    // Advance timer to drain all
    clock.advance(Duration::from_micros(100));

    let mut queue = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }))
    .unwrap()
    .unwrap();

    // Should come out in order 1, 2, 3, ..., 10
    let mut collected = Vec::new();
    while let Some(entry) = queue.pop_front() {
        collected.push(entry.meta);
    }

    assert_eq!(collected.len(), 10);
    for (idx, expected) in (1..11).enumerate() {
        assert_eq!(
            collected[idx], expected as u16,
            "Entry at position {idx} should be {expected}"
        );
    }
}

#[test]
fn test_empty_wheel_fast_path() {
    let clock = Clock::new(Duration::from_micros(1000));
    let channel = TestChannel::new();
    let mut wheel: TestWheel = Wheel::new(&channel, clock.timer());

    // With empty wheel, advancing time should be fast (no slot scanning)
    let start_tick = wheel.current_tick;

    // Advance timer by 1000 ticks
    clock.advance(Duration::from_micros(1000));

    // Poll should handle the time advance efficiently
    let result = poll_once(core::future::poll_fn(|cx| {
        wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))
    }));
    assert!(result.is_none());

    // Current tick should have advanced
    assert_eq!(
        wheel.current_tick,
        start_tick + 1000,
        "Wheel should advance time even when empty"
    );
}

/// Oracle-based fuzz test: compare the wheel against a BTreeMap oracle.
///
/// For random insertion patterns, we verify that:
/// 1. Entries are retrieved in correct order by time
/// 2. All entries are retrieved (no lost entries)
/// 3. Len tracking is accurate
#[test]
fn fuzz_oracle_comparison() {
    use bolero::check;

    const MAX_OFFSET: u64 = 20_000_000;

    check!()
        .with_type::<(u32, Vec<u32>)>()
        .with_test_time(core::time::Duration::from_secs(10))
        .for_each(|(start_offset, offsets)| {
            let base_us = 1_000 + (*start_offset as u64) * 256;
            let start = precision::Timestamp {
                nanos: Duration::from_micros(base_us).as_nanos() as u64,
            };
            let clock = Clock::new(Duration::from_micros(base_us));
            let channel = TestChannel::new();
            let mut wheel: TestWheel =
                Wheel::new(&channel, clock.timer());

            let start_tick =
                timestamp_to_tick(start, 1);

            // Oracle: BTreeMap<tick, Vec<meta>> preserves insertion order per tick
            let mut oracle: BTreeMap<u64, Vec<u16>> = BTreeMap::new();

            // Insert entries with varying offsets (always at least 1µs in the future)
            let mut batch = Queue::new();
            for (i, &raw_offset) in offsets.iter().enumerate() {
                let offset_us = ((raw_offset as u64) % MAX_OFFSET) + 1;
                let target_tick = start_tick + offset_us;
                let time = start + Duration::from_micros(offset_us);
                let effective_tick = target_tick.max(start_tick);

                let meta = i as u16;
                batch.push_back(
                    TestEntry {
                        meta,
                        transmission_time: Some(time),
                    }
                    .into(),
                );
                oracle.entry(effective_tick).or_default().push(meta);
            }

            let expected_count = offsets.len();
            channel.send_batch(batch);

            // Poll to insert entries
            let _ = poll_once(core::future::poll_fn(|cx| wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))));

            // Verify len tracking
            assert_eq!(
                wheel.len, expected_count,
                "Wheel len should match number of inserted entries"
            );

            // Step through ticks and verify entries match oracle
            let mut current_tick = start_tick;
            let end_tick = start_tick + MAX_OFFSET;
            let mut total_collected = 0;

            while current_tick <= end_tick && !oracle.is_empty() {
                let current_time =
                    tick_to_timestamp(current_tick, 1);

                // Update timer to current time
                clock.set(current_time);

                // Poll to get entries
                let poll_result =
                    poll_once(core::future::poll_fn(|cx| wheel.poll_recv(cx, &mut channel::Budget::new(usize::MAX))));

                let mut wheel_entries = Vec::new();
                if let Some(Some(mut queue)) = poll_result {
                    while let Some(entry) = queue.pop_front() {
                        wheel_entries.push(entry.meta);
                        total_collected += 1;
                    }
                }

                // Check against oracle for this tick
                if let Some(oracle_entries) = oracle.remove(&current_tick) {
                    assert_eq!(
                        wheel_entries.len(),
                        oracle_entries.len(),
                        "At tick {current_tick}: wheel returned {} entries, oracle expected {}",
                        wheel_entries.len(),
                        oracle_entries.len()
                    );

                    // Entries should match in order (FIFO within same tick)
                    for (w, o) in wheel_entries.iter().zip(oracle_entries.iter()) {
                        assert_eq!(
                            w, o,
                            "At tick {current_tick}: entry mismatch. Wheel: {w}, Oracle: {o}"
                        );
                    }
                } else {
                    assert!(
                        wheel_entries.is_empty(),
                        "At tick {current_tick}: wheel returned {} entries but oracle expected none",
                        wheel_entries.len()
                    );
                }

                current_tick += 1;
            }

            // Verify all entries were collected
            assert_eq!(
                total_collected, expected_count,
                "Should collect all inserted entries. Got {total_collected}, expected {expected_count}"
            );

            // Verify len is back to 0
            assert_eq!(
                wheel.len, 0,
                "Wheel len should be 0 after draining all entries"
            );
        });
}

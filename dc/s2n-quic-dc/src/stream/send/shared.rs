// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{
        packet_number,
        send::{
            application::transmission, buffer, error::Error, flow, path, queue::Queue,
            state::Transmission,
        },
        shared::ShutdownKind,
    },
    task::waker::worker::Waker as WorkerWaker,
};
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use crossbeam_queue::SegQueue;
use s2n_quic_core::recovery::bandwidth::Bandwidth;
use tracing::trace;

#[derive(Debug)]
pub struct Message {
    /// The event being submitted to the worker
    pub event: Event,
}

#[derive(Debug)]
pub enum Event {
    Shutdown { queue: Queue, kind: ShutdownKind },
}

pub struct State {
    pub flow: flow::non_blocking::State,
    pub packet_number: packet_number::Counter,
    pub path: path::State,
    pub worker_waker: WorkerWaker,
    bandwidth: AtomicU64,
    /// A channel sender for pushing transmission information to the worker task
    ///
    /// We use an unbounded sender since we already rely on flow control to apply backpressure
    worker_queue: SegQueue<Message>,
    pub application_transmission_queue: transmission::Queue<buffer::Segment>,
    pub segment_alloc: buffer::Allocator,
}

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("send::shared::State")
            .field("flow", &self.flow)
            .field("packet_number", &self.packet_number)
            .field("path", &self.path)
            .finish()
    }
}

impl State {
    #[inline]
    pub fn new(
        flow: flow::non_blocking::State,
        path: path::Info,
        bandwidth: Option<Bandwidth>,
    ) -> Self {
        let path = path::State::new(path);
        let bandwidth = bandwidth.map(|v| v.serialize()).unwrap_or(u64::MAX).into();
        Self {
            flow,
            packet_number: Default::default(),
            path,
            bandwidth,
            // this will get set once the waker spawns
            worker_waker: Default::default(),
            worker_queue: Default::default(),
            application_transmission_queue: Default::default(),
            segment_alloc: Default::default(),
        }
    }

    #[inline]
    pub fn bandwidth(&self) -> Bandwidth {
        Bandwidth::deserialize(self.bandwidth.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn set_bandwidth(&self, value: Bandwidth) {
        self.bandwidth.store(value.serialize(), Ordering::Relaxed);
    }

    #[inline]
    pub fn pop_worker_message(&self) -> Option<Message> {
        self.worker_queue.pop()
    }

    #[inline]
    pub fn push_to_worker(&self, transmissions: Vec<Transmission>) -> Result<(), Error> {
        trace!(event = "transmission", len = transmissions.len());
        self.application_transmission_queue
            .push_batch(transmissions);

        self.worker_waker.wake();

        Ok(())
    }

    pub fn on_prune(&self) {
        self.shutdown(Default::default(), ShutdownKind::Pruned);
    }

    #[inline]
    pub fn shutdown(&self, queue: Queue, kind: ShutdownKind) {
        trace!(event = "shutdown", queue = queue.accepted_len(), ?kind);
        let message = Message {
            event: Event::Shutdown { queue, kind },
        };
        self.worker_queue.push(message);
        self.worker_waker.wake();
    }
}

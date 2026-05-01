// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::stream::PacketSpace,
    stream::{
        packet_number,
        send::{flow, path, queue::Queue, state::transmission},
        shared::{CompletionQueue, Half, ShutdownKind},
    },
    task::waker::worker,
};
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use crossbeam_queue::SegQueue;
use s2n_quic_core::recovery::bandwidth::Bandwidth;
use std::sync::Weak;
use tracing::trace;

#[derive(Debug)]
pub struct Message {
    /// The event being submitted to the worker
    pub event: Event,
}

#[derive(Debug)]
pub enum Event {
    Shutdown {
        kind: ShutdownKind,
        queue: Queue,
        /// Indicates the application already transmitted the fin as part of its stream data
        fin_sent: bool,
    },
    KeepAlive {
        enabled: bool,
    },
}

pub struct State {
    pub flow: flow::non_blocking::State,
    pub packet_number: packet_number::Counter,
    pub path: path::State,
    bandwidth: AtomicU64,
    /// A channel sender for pushing transmission information to the worker task
    ///
    /// We use an unbounded sender since we already rely on flow control to apply backpressure
    worker_queue: SegQueue<Message>,
    pub transmission_queue: transmission::Queue,
    pub completion_handle: CompletionQueue<Weak<dyn transmission::Notify>>,
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
            worker_queue: Default::default(),
            transmission_queue: Default::default(),
            completion_handle: CompletionQueue::uninit(),
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

    pub fn keep_alive(&self, enabled: bool, waker: &worker::Waker) {
        self.worker_queue.push(Message {
            event: Event::KeepAlive { enabled },
        });
        waker.wake();
    }

    #[inline]
    pub fn shutdown(
        &self,
        kind: ShutdownKind,
        queue: Queue,
        fin_sent: bool,
        waker: &worker::Waker,
    ) {
        trace!(event = "shutdown", ?kind, fin_sent);
        let message = Message {
            event: Event::Shutdown {
                kind,
                queue,
                fin_sent,
            },
        };
        self.worker_queue.push(message);
        waker.wake();
    }

    /// Sets the error flag on the flow state so the write application path detects the error.
    #[inline]
    pub fn set_error_flag(&self) {
        self.flow.set_error_flag();
    }

    pub fn alloc_transmission(&self, packet_space: PacketSpace) -> transmission::Entry {
        let completion_queue = || unsafe { self.completion_handle.load() };
        let mut entry = self.transmission_queue.alloc_entry(completion_queue);
        entry.meta.packet_space = packet_space;
        entry.meta.half = Half::Write;
        entry
    }
}

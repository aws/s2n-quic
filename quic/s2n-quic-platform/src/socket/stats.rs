// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
    task::Poll,
};
use s2n_quic_core::{
    event::{self, EndpointPublisher},
    io::event_loop,
};
use std::{
    collections::VecDeque,
    ffi::c_int,
    io,
    sync::{Arc, Mutex},
};

const ERROR_QUEUE_CAP: usize = 256;
type Error = c_int;

pub fn channel() -> (Sender, Receiver) {
    channel_with_rx_socket_count(0)
}

pub fn channel_with_rx_socket_count(rx_socket_count: usize) -> (Sender, Receiver) {
    let per_socket_rx = (0..rx_socket_count)
        .map(|_| AtomicU64::new(0))
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let state = Arc::new(State {
        send: Stats::default(),
        recv: Stats::default(),
        per_socket_rx,
    });

    let sender = Sender {
        state: state.clone(),
        socket_index: None,
    };

    let recv = Receiver {
        state,
        pending_errors: VecDeque::with_capacity(ERROR_QUEUE_CAP),
    };

    (sender, recv)
}

#[derive(Clone)]
pub struct Sender {
    state: Arc<State>,
    socket_index: Option<usize>,
}

impl fmt::Debug for Sender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sender").finish_non_exhaustive()
    }
}

impl Sender {
    #[inline]
    pub fn send(&self) -> &Stats {
        &self.state.send
    }

    #[inline]
    pub fn recv(&self) -> &Stats {
        &self.state.recv
    }

    /// Returns a new `Sender` with the given socket index set.
    ///
    /// When set, recv operations will also be tracked per-socket, enabling
    /// `PlatformRxSocketStats` events to be emitted for each socket.
    #[inline]
    pub fn with_socket_index(mut self, index: usize) -> Self {
        self.socket_index = Some(index);
        self
    }

    /// Records that packets were received on this sender's socket.
    ///
    /// This should be called after a successful recv operation to track
    /// per-socket packet counts.
    #[inline]
    pub fn on_recv_socket_packets(&self, count: usize) {
        if let Some(index) = self.socket_index {
            if let Some(counter) = self.state.per_socket_rx.get(index) {
                counter.fetch_add(count as u64, Ordering::Relaxed);
            }
        }
    }
}

pub struct Receiver {
    state: Arc<State>,
    pending_errors: VecDeque<Error>,
}

impl fmt::Debug for Receiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Receiver").finish_non_exhaustive()
    }
}

impl event_loop::Stats for Receiver {
    #[inline]
    fn publish<P: EndpointPublisher>(&mut self, publisher: &mut P) {
        self.state.send.publish(
            publisher,
            &mut self.pending_errors,
            |publisher, errno| {
                publisher.on_platform_tx_error(event::builder::PlatformTxError { errno });
            },
            |publisher, metrics| publisher.on_platform_tx(metrics.into()),
        );
        self.state.recv.publish(
            publisher,
            &mut self.pending_errors,
            |publisher, errno| {
                publisher.on_platform_rx_error(event::builder::PlatformRxError { errno });
            },
            |publisher, metrics| publisher.on_platform_rx(metrics.into()),
        );

        // Emit per-socket rx stats
        for (id, counter) in self.state.per_socket_rx.iter().enumerate() {
            let count = counter.swap(0, Ordering::Relaxed);
            if count > 0 {
                publisher.on_platform_rx_socket_stats(event::builder::PlatformRxSocketStats {
                    id,
                    count: count as usize,
                });
            }
        }
    }
}

#[derive(Default)]
struct State {
    send: Stats,
    recv: Stats,
    per_socket_rx: Box<[AtomicU64]>,
}

pub struct Stats {
    syscalls: AtomicU64,
    blocked: AtomicU64,
    packets: AtomicU64,
    errors: Mutex<VecDeque<Error>>,
    total_errors: AtomicU64,
    dropped_errors: AtomicU64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            syscalls: Default::default(),
            blocked: Default::default(),
            packets: Default::default(),
            errors: Mutex::new(VecDeque::with_capacity(ERROR_QUEUE_CAP)),
            total_errors: Default::default(),
            dropped_errors: Default::default(),
        }
    }
}

impl fmt::Debug for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stats").finish_non_exhaustive()
    }
}

impl Stats {
    #[inline]
    pub fn on_operation<T, F>(&self, res: &Poll<io::Result<T>>, count_packets: F)
    where
        F: FnOnce(&T) -> usize,
    {
        match res {
            Poll::Ready(res) => {
                self.on_operation_result(res, count_packets);
            }
            Poll::Pending => {
                self.on_operation_pending();
            }
        }
    }

    #[inline]
    pub fn on_operation_result<T, F>(&self, res: &io::Result<T>, count_packets: F)
    where
        F: FnOnce(&T) -> usize,
    {
        match res {
            Ok(value) => {
                let packets = count_packets(value);
                self.on_operation_ready(packets);
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                self.on_operation_pending();
            }
            Err(err) => {
                self.on_operation_ready(0);
                if let Some(err) = err.raw_os_error() {
                    self.on_error(err);
                } else {
                    self.dropped_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    #[inline]
    pub fn on_operation_ready(&self, packets: usize) {
        if packets > 0 {
            self.packets.fetch_add(packets as _, Ordering::Relaxed);
        }
        self.syscalls.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn on_operation_pending(&self) {
        self.syscalls.fetch_add(1, Ordering::Relaxed);
        self.blocked.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn on_error(&self, error: Error) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);

        let mut did_drop = false;
        if let Ok(mut queue) = self.errors.try_lock() {
            // drop old errors
            if queue.len() == ERROR_QUEUE_CAP {
                let _ = queue.pop_front();
                did_drop = true;
            }

            queue.push_back(error);
        } else {
            did_drop = true;
        };

        if did_drop {
            self.dropped_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    fn publish<P, OnError, OnMetrics>(
        &self,
        publisher: &mut P,
        errors: &mut VecDeque<Error>,
        on_error: OnError,
        on_metrics: OnMetrics,
    ) where
        OnError: Fn(&mut P, Error),
        OnMetrics: Fn(&mut P, Metrics),
    {
        core::mem::swap(&mut *self.errors.lock().unwrap(), errors);

        for error in errors.drain(..) {
            on_error(publisher, error);
        }

        let metrics = self.metrics();
        if metrics.syscalls > 0 {
            on_metrics(publisher, metrics);
        }
    }

    #[inline]
    fn metrics(&self) -> Metrics {
        macro_rules! take {
            ($field:ident) => {{
                let value = self.$field.swap(0, Ordering::Relaxed);
                value.try_into().unwrap_or(usize::MAX)
            }};
        }

        let packets = take!(packets);
        let syscalls = take!(syscalls);
        let blocked_syscalls = take!(blocked);
        let total_errors = take!(total_errors);
        let dropped_errors = take!(dropped_errors);

        Metrics {
            packets,
            syscalls,
            blocked_syscalls,
            total_errors,
            dropped_errors,
        }
    }
}

#[derive(Clone, Copy)]
struct Metrics {
    packets: usize,
    syscalls: usize,
    blocked_syscalls: usize,
    total_errors: usize,
    dropped_errors: usize,
}

impl From<Metrics> for event::builder::PlatformRx {
    fn from(value: Metrics) -> Self {
        Self {
            count: value.packets,
            syscalls: value.syscalls,
            blocked_syscalls: value.blocked_syscalls,
            total_errors: value.total_errors,
            dropped_errors: value.dropped_errors,
        }
    }
}

impl From<Metrics> for event::builder::PlatformTx {
    fn from(value: Metrics) -> Self {
        Self {
            count: value.packets,
            syscalls: value.syscalls,
            blocked_syscalls: value.blocked_syscalls,
            total_errors: value.total_errors,
            dropped_errors: value.dropped_errors,
        }
    }
}

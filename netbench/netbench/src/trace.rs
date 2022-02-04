// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::operation::ConnectionOperation;
use core::{fmt, time::Duration};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

pub trait Trace {
    #[inline(always)]
    fn exec(&mut self, op: &ConnectionOperation) {
        let _ = op;
    }

    #[inline(always)]
    fn enter(&mut self, scope: usize, thread: usize) {
        let _ = scope;
        let _ = thread;
    }

    #[inline(always)]
    fn exit(&mut self) {}

    #[inline(always)]
    fn send(&mut self, stream_id: u64, len: u64) {
        let _ = stream_id;
        let _ = len;
    }

    #[inline(always)]
    fn receive(&mut self, stream_id: u64, len: u64) {
        let _ = stream_id;
        let _ = len;
    }

    #[inline(always)]
    fn accept(&mut self, stream_id: u64) {
        let _ = stream_id;
    }

    #[inline(always)]
    fn trace(&mut self, id: u64) {
        let _ = id;
    }
}

#[derive(Clone, Debug, Default)]
pub struct Disabled(());

impl Trace for Disabled {}

#[derive(Clone, Debug)]
pub struct Logger<'a> {
    id: u64,
    traces: &'a [String],
    scope: Vec<(usize, usize)>,
}

impl<'a> Logger<'a> {
    pub fn new(id: u64, traces: &'a [String]) -> Self {
        Self {
            id,
            traces,
            scope: vec![],
        }
    }

    fn log(&self, v: impl fmt::Display) {
        use std::io::Write;

        let out = std::io::stdout();
        let mut out = out.lock();
        let _ = write!(out, "{}:", self.id);
        for (scope, thread) in self.scope.iter() {
            let _ = write!(out, "{}.{}:", scope, thread);
        }
        let _ = writeln!(out, "{}", v);
    }
}

impl Trace for Logger<'_> {
    #[inline(always)]
    fn exec(&mut self, op: &ConnectionOperation) {
        self.log(format_args!("exec: {:?}", op));
    }

    #[inline(always)]
    fn enter(&mut self, scope: usize, thread: usize) {
        self.scope.push((scope, thread));
    }

    #[inline(always)]
    fn exit(&mut self) {
        self.scope.pop();
    }

    #[inline(always)]
    fn send(&mut self, stream_id: u64, len: u64) {
        self.log(format_args!("send: stream={}, len={}", stream_id, len));
    }

    #[inline(always)]
    fn receive(&mut self, stream_id: u64, len: u64) {
        self.log(format_args!("recv: stream={}, len={}", stream_id, len));
    }

    #[inline(always)]
    fn accept(&mut self, stream_id: u64) {
        self.log(format_args!("accept: stream={}", stream_id));
    }

    #[inline(always)]
    fn trace(&mut self, id: u64) {
        if let Some(msg) = self.traces.get(id as usize) {
            self.log(format_args!("trace: {}", msg));
        } else {
            self.log(format_args!("trace: id={}", id));
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Throughput<Counter> {
    rx: Counter,
    tx: Counter,
}

impl Throughput<Arc<AtomicU64>> {
    pub fn take(&self) -> Throughput<u64> {
        Throughput {
            rx: self.rx.swap(0, Ordering::Relaxed),
            tx: self.tx.swap(0, Ordering::Relaxed),
        }
    }

    pub fn reporter(&self, freq: Duration) -> ReporterHandle {
        let handle = ReporterHandle::default();
        let values = self.clone();
        let r_handle = handle.clone();
        tokio::spawn(async move {
            while !r_handle.0.fetch_or(false, Ordering::Relaxed) {
                tokio::time::sleep(freq).await;
                let v = values.take();
                eprintln!("{:?}", v);
            }
        });

        handle
    }
}

impl Trace for Throughput<Arc<AtomicU64>> {
    fn send(&mut self, _stream_id: u64, len: u64) {
        self.tx.fetch_add(len, Ordering::Relaxed);
    }

    fn receive(&mut self, _stream_id: u64, len: u64) {
        self.rx.fetch_add(len, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReporterHandle(Arc<AtomicBool>);

impl Drop for ReporterHandle {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

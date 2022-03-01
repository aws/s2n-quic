// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{operation as op, timer::Timestamp, units::*};
use core::fmt;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

pub trait Trace {
    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        let _ = now;
        let _ = op;
    }

    #[inline(always)]
    fn enter(&mut self, now: Timestamp, scope: usize, thread: usize) {
        let _ = now;
        let _ = scope;
        let _ = thread;
    }

    #[inline(always)]
    fn exit(&mut self, now: Timestamp) {
        let _ = now;
    }

    #[inline(always)]
    fn send(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        let _ = now;
        let _ = stream_id;
        let _ = len;
    }

    #[inline(always)]
    fn send_finish(&mut self, now: Timestamp, stream_id: u64) {
        let _ = now;
        let _ = stream_id;
    }

    #[inline(always)]
    fn receive(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        let _ = now;
        let _ = stream_id;
        let _ = len;
    }

    #[inline(always)]
    fn receive_finish(&mut self, now: Timestamp, stream_id: u64) {
        let _ = now;
        let _ = stream_id;
    }

    #[inline(always)]
    fn accept(&mut self, now: Timestamp, stream_id: u64) {
        let _ = now;
        let _ = stream_id;
    }

    #[inline(always)]
    fn open(&mut self, now: Timestamp, stream_id: u64) {
        let _ = now;
        let _ = stream_id;
    }

    #[inline(always)]
    fn trace(&mut self, now: Timestamp, id: u64) {
        let _ = now;
        let _ = id;
    }

    #[inline(always)]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        let _ = now;
        let _ = id;
    }
}

impl<A: Trace, B: Trace> Trace for (A, B) {
    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        self.0.exec(now, op);
        self.1.exec(now, op);
    }

    #[inline(always)]
    fn enter(&mut self, now: Timestamp, scope: usize, thread: usize) {
        self.0.enter(now, scope, thread);
        self.1.enter(now, scope, thread);
    }

    #[inline(always)]
    fn exit(&mut self, now: Timestamp) {
        self.0.exit(now);
        self.1.exit(now);
    }

    #[inline(always)]
    fn send(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        self.0.send(now, stream_id, len);
        self.1.send(now, stream_id, len);
    }

    #[inline(always)]
    fn send_finish(&mut self, now: Timestamp, stream_id: u64) {
        self.0.send_finish(now, stream_id);
        self.1.send_finish(now, stream_id);
    }

    #[inline(always)]
    fn receive(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        self.0.receive(now, stream_id, len);
        self.1.receive(now, stream_id, len);
    }

    #[inline(always)]
    fn receive_finish(&mut self, now: Timestamp, stream_id: u64) {
        self.0.receive_finish(now, stream_id);
        self.1.receive_finish(now, stream_id);
    }

    #[inline(always)]
    fn accept(&mut self, now: Timestamp, stream_id: u64) {
        self.0.accept(now, stream_id);
        self.1.accept(now, stream_id)
    }

    #[inline(always)]
    fn open(&mut self, now: Timestamp, stream_id: u64) {
        self.0.open(now, stream_id);
        self.1.open(now, stream_id)
    }

    #[inline(always)]
    fn trace(&mut self, now: Timestamp, id: u64) {
        self.0.trace(now, id);
        self.1.trace(now, id)
    }

    #[inline]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        self.0.unpark(now, id);
        self.1.unpark(now, id);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Disabled(());

impl Trace for Disabled {}

#[derive(Clone, Debug)]
pub struct Logger<'a, O: Output> {
    id: u64,
    traces: &'a [String],
    scope: Vec<(usize, usize)>,
    output: O,
}

pub type MemoryLogger<'a> = Logger<'a, std::io::Cursor<Vec<u8>>>;
pub type StdioLogger<'a> = Logger<'a, std::io::BufWriter<std::io::Stdout>>;

impl<'a, O: Output> Logger<'a, O> {
    pub fn new(id: u64, traces: &'a [String]) -> Self {
        Self {
            id,
            traces,
            scope: vec![],
            output: O::new(),
        }
    }

    fn log(&mut self, now: Timestamp, v: impl fmt::Display) {
        let id = self.id;
        let scope = &self.scope;
        let _ = self.output.write(|out| {
            use std::io::Write;
            write!(out, "{}: ", now)?;
            write!(out, "{}:", id)?;
            for (scope, thread) in scope.iter() {
                write!(out, "{}.{}:", scope, thread)?;
            }
            writeln!(out, "{}", v)?;
            Ok(())
        });
    }
}

impl<'a> Logger<'a, std::io::Cursor<Vec<u8>>> {
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(self.output.get_ref()).ok()
    }
}

pub trait Output {
    type Io: std::io::Write;

    fn new() -> Self;
    fn write<F: FnOnce(&mut Self::Io) -> std::io::Result<()>>(
        &mut self,
        f: F,
    ) -> std::io::Result<()>;
}

impl Output for std::io::BufWriter<std::io::Stdout> {
    type Io = Self;

    fn new() -> Self {
        std::io::BufWriter::new(std::io::stdout())
    }

    fn write<F: FnOnce(&mut Self::Io) -> std::io::Result<()>>(
        &mut self,
        f: F,
    ) -> std::io::Result<()> {
        f(self)?;
        use std::io::Write;
        self.flush()?;
        Ok(())
    }
}

impl Output for std::io::Cursor<Vec<u8>> {
    type Io = Self;

    fn new() -> Self {
        std::io::Cursor::new(vec![])
    }

    fn write<F: FnOnce(&mut Self::Io) -> std::io::Result<()>>(
        &mut self,
        f: F,
    ) -> std::io::Result<()> {
        f(self)?;
        Ok(())
    }
}

impl<O: Output> Trace for Logger<'_, O> {
    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        self.log(now, format_args!("exec: {:?}", op));
    }

    #[inline(always)]
    fn enter(&mut self, _now: Timestamp, scope: usize, thread: usize) {
        self.scope.push((scope, thread));
    }

    #[inline(always)]
    fn exit(&mut self, _now: Timestamp) {
        self.scope.pop();
    }

    #[inline(always)]
    fn send(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        self.log(now, format_args!("send: stream={}, len={}", stream_id, len));
    }

    #[inline(always)]
    fn send_finish(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("send finish: stream={}", stream_id));
    }

    #[inline(always)]
    fn receive(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        self.log(now, format_args!("recv: stream={}, len={}", stream_id, len));
    }

    #[inline(always)]
    fn receive_finish(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("recv finish: stream={}", stream_id));
    }

    #[inline(always)]
    fn accept(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("accept: stream={}", stream_id));
    }

    #[inline(always)]
    fn open(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("open: stream={}", stream_id));
    }

    #[inline(always)]
    fn trace(&mut self, now: Timestamp, id: u64) {
        if let Some(msg) = self.traces.get(id as usize) {
            self.log(now, format_args!("trace: {}", msg));
        } else {
            self.log(now, format_args!("trace: id={}", id));
        }
    }

    #[inline(always)]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        self.log(now, format_args!("unpark: {}", id));
    }
}

#[derive(Clone, Debug, Default)]
pub struct Throughput<Counter = Arc<AtomicU64>> {
    rx: Counter,
    tx: Counter,
}

impl Throughput {
    pub fn take(&self) -> Throughput<Byte> {
        Throughput {
            rx: self.rx.swap(0, Ordering::Relaxed).bytes(),
            tx: self.tx.swap(0, Ordering::Relaxed).bytes(),
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
                eprintln!("{}", v / freq);
            }
        });

        handle
    }
}

impl core::ops::Div<Duration> for Throughput<Byte> {
    type Output = Throughput<Rate>;

    fn div(self, duration: Duration) -> Throughput<Rate> {
        Throughput {
            rx: self.rx / duration,
            tx: self.tx / duration,
        }
    }
}

impl fmt::Display for Throughput<Rate> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "throughput: rx={}, tx={}", self.rx, self.tx)
    }
}

impl Trace for Throughput {
    fn send(&mut self, _now: Timestamp, _stream_id: u64, len: u64) {
        self.tx.fetch_add(len, Ordering::Relaxed);
    }

    fn receive(&mut self, _now: Timestamp, _stream_id: u64, len: u64) {
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

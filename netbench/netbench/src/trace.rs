// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{operation as op, timer::Timestamp, units::*};
use core::fmt;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

mod usdt;
pub use self::usdt::Usdt;

pub trait Trace {
    #[inline(always)]
    fn enter_connection(&mut self, id: u64) {
        let _ = id;
    }

    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        let _ = now;
        let _ = op;
    }

    #[inline(always)]
    fn exec_client(&mut self, now: Timestamp, op: &op::Client) {
        let _ = now;
        let _ = op;
    }

    #[inline(always)]
    fn enter(&mut self, now: Timestamp, scope: u64, thread: usize) {
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
    fn profile(&mut self, now: Timestamp, id: u64, time: Duration) {
        let _ = now;
        let _ = id;
        let _ = time;
    }

    #[inline(always)]
    fn park(&mut self, now: Timestamp, id: u64) {
        let _ = now;
        let _ = id;
    }

    #[inline(always)]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        let _ = now;
        let _ = id;
    }

    #[inline(always)]
    fn connect(&mut self, now: Timestamp, connection_id: u64, time: Duration) {
        let _ = now;
        let _ = connection_id;
        let _ = time;
    }
}

impl<A: Trace, B: Trace> Trace for (A, B) {
    #[inline(always)]
    fn enter_connection(&mut self, id: u64) {
        self.0.enter_connection(id);
        self.1.enter_connection(id);
    }

    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        self.0.exec(now, op);
        self.1.exec(now, op);
    }

    #[inline(always)]
    fn exec_client(&mut self, now: Timestamp, op: &op::Client) {
        self.0.exec_client(now, op);
        self.1.exec_client(now, op);
    }

    #[inline(always)]
    fn enter(&mut self, now: Timestamp, scope: u64, thread: usize) {
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

    #[inline(always)]
    fn profile(&mut self, now: Timestamp, id: u64, time: Duration) {
        self.0.profile(now, id, time);
        self.1.profile(now, id, time)
    }

    #[inline]
    fn park(&mut self, now: Timestamp, id: u64) {
        self.0.park(now, id);
        self.1.park(now, id);
    }

    #[inline]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        self.0.unpark(now, id);
        self.1.unpark(now, id);
    }

    #[inline(always)]
    fn connect(&mut self, now: Timestamp, connection_id: u64, time: Duration) {
        self.0.connect(now, connection_id, time);
        self.1.connect(now, connection_id, time);
    }
}

impl<T: Trace> Trace for Option<T> {
    #[inline(always)]
    fn enter_connection(&mut self, id: u64) {
        if let Some(t) = self.as_mut() {
            t.enter_connection(id);
        }
    }

    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        if let Some(t) = self.as_mut() {
            t.exec(now, op);
        }
    }

    #[inline(always)]
    fn exec_client(&mut self, now: Timestamp, op: &op::Client) {
        if let Some(t) = self.as_mut() {
            t.exec_client(now, op);
        }
    }

    #[inline(always)]
    fn enter(&mut self, now: Timestamp, scope: u64, thread: usize) {
        if let Some(t) = self.as_mut() {
            t.enter(now, scope, thread);
        }
    }

    #[inline(always)]
    fn exit(&mut self, now: Timestamp) {
        if let Some(t) = self.as_mut() {
            t.exit(now);
        }
    }

    #[inline(always)]
    fn send(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        if let Some(t) = self.as_mut() {
            t.send(now, stream_id, len);
        }
    }

    #[inline(always)]
    fn send_finish(&mut self, now: Timestamp, stream_id: u64) {
        if let Some(t) = self.as_mut() {
            t.send_finish(now, stream_id);
        }
    }

    #[inline(always)]
    fn receive(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        if let Some(t) = self.as_mut() {
            t.receive(now, stream_id, len);
        }
    }

    #[inline(always)]
    fn receive_finish(&mut self, now: Timestamp, stream_id: u64) {
        if let Some(t) = self.as_mut() {
            t.receive_finish(now, stream_id);
        }
    }

    #[inline(always)]
    fn accept(&mut self, now: Timestamp, stream_id: u64) {
        if let Some(t) = self.as_mut() {
            t.accept(now, stream_id);
        }
    }

    #[inline(always)]
    fn open(&mut self, now: Timestamp, stream_id: u64) {
        if let Some(t) = self.as_mut() {
            t.open(now, stream_id);
        }
    }

    #[inline(always)]
    fn trace(&mut self, now: Timestamp, id: u64) {
        if let Some(t) = self.as_mut() {
            t.trace(now, id);
        }
    }

    #[inline(always)]
    fn profile(&mut self, now: Timestamp, id: u64, time: Duration) {
        if let Some(t) = self.as_mut() {
            t.profile(now, id, time);
        }
    }

    #[inline]
    fn park(&mut self, now: Timestamp, id: u64) {
        if let Some(t) = self.as_mut() {
            t.park(now, id);
        }
    }

    #[inline]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        if let Some(t) = self.as_mut() {
            t.unpark(now, id);
        }
    }

    #[inline]
    fn connect(&mut self, now: Timestamp, connection_id: u64, time: Duration) {
        if let Some(t) = self.as_mut() {
            t.connect(now, connection_id, time);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Disabled(());

impl Trace for Disabled {}

#[derive(Debug)]
pub struct Logger<O: Output> {
    id: u64,
    traces: Arc<Vec<String>>,
    scope: Vec<(u64, usize)>,
    output: O,
    verbose: bool,
}

pub type MemoryLogger = Logger<std::io::Cursor<Vec<u8>>>;
pub type StdioLogger = Logger<std::io::BufWriter<std::io::Stdout>>;

impl<O: Output> Logger<O> {
    pub fn new(traces: Arc<Vec<String>>) -> Self {
        Self {
            id: 0,
            traces,
            scope: vec![],
            output: O::new(),
            verbose: false,
        }
    }

    pub fn verbose(&mut self, enabled: bool) {
        self.verbose = enabled;
    }

    fn log(&mut self, now: Timestamp, v: impl fmt::Display) {
        self.output.log(self.id, &self.scope, self.verbose, now, v)
    }
}

impl MemoryLogger {
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(self.output.get_ref()).ok()
    }
}

impl Clone for StdioLogger {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            traces: self.traces.clone(),
            scope: vec![],
            output: Output::new(),
            verbose: self.verbose,
        }
    }
}

pub trait Output {
    type Io: std::io::Write;

    fn new() -> Self;

    fn write<F: FnOnce(&mut Self::Io) -> std::io::Result<()>>(
        &mut self,
        f: F,
    ) -> std::io::Result<()>;

    fn log(
        &mut self,
        id: u64,
        scope: &[(u64, usize)],
        verbose: bool,
        now: Timestamp,
        v: impl fmt::Display,
    ) {
        let _ = self.write(|out| {
            use std::io::Write;
            write!(out, "{} ", now)?;

            write!(out, "[{}", id)?;
            if verbose {
                for (scope, thread) in scope.iter() {
                    write!(out, ":{}.{}", scope, thread)?;
                }
            }
            write!(out, "] ")?;

            writeln!(out, "{}", v)?;
            Ok(())
        });
    }
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

impl<O: Output> Trace for Logger<O> {
    #[inline(always)]
    fn enter_connection(&mut self, id: u64) {
        self.id = id;
    }

    #[inline(always)]
    fn exec(&mut self, now: Timestamp, op: &op::Connection) {
        if self.verbose {
            self.log(now, format_args!("exec: {:?}", op));
        }
    }

    #[inline(always)]
    fn exec_client(&mut self, now: Timestamp, op: &op::Client) {
        if self.verbose {
            self.log(now, format_args!("exec: {:?}", op));
        }
    }

    #[inline(always)]
    fn enter(&mut self, _now: Timestamp, scope: u64, thread: usize) {
        self.scope.push((scope, thread));
    }

    #[inline(always)]
    fn exit(&mut self, _now: Timestamp) {
        self.scope.pop();
    }

    #[inline(always)]
    fn send(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        self.log(now, format_args!("send[{}]={}", stream_id, len));
    }

    #[inline(always)]
    fn send_finish(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("sfin[{}]", stream_id));
    }

    #[inline(always)]
    fn receive(&mut self, now: Timestamp, stream_id: u64, len: u64) {
        self.log(now, format_args!("recv[{}]={}", stream_id, len));
    }

    #[inline(always)]
    fn receive_finish(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("rfin[{}]", stream_id));
    }

    #[inline(always)]
    fn accept(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("acpt[{}]", stream_id));
    }

    #[inline(always)]
    fn open(&mut self, now: Timestamp, stream_id: u64) {
        self.log(now, format_args!("open[{}]", stream_id));
    }

    #[inline(always)]
    fn trace(&mut self, now: Timestamp, id: u64) {
        if let Some(msg) = self.traces.get(id as usize).filter(|_| self.verbose) {
            let id = self.id;
            let scope = &self.scope;
            let verbose = self.verbose;
            self.output
                .log(id, scope, verbose, now, format_args!("trce[{}]", msg));
        } else {
            self.log(now, format_args!("trce[{}]", id));
        }
    }

    #[inline(always)]
    fn profile(&mut self, now: Timestamp, id: u64, time: Duration) {
        if let Some(msg) = self.traces.get(id as usize).filter(|_| self.verbose) {
            let id = self.id;
            let scope = &self.scope;
            let verbose = self.verbose;
            self.output.log(
                id,
                scope,
                verbose,
                now,
                format_args!("prof[{}]={:?}", msg, time),
            );
        } else {
            self.log(now, format_args!("prof[{}]={:?}", id, time));
        }
    }

    #[inline(always)]
    fn park(&mut self, now: Timestamp, id: u64) {
        self.log(now, format_args!("park[{}]", id));
    }

    #[inline(always)]
    fn unpark(&mut self, now: Timestamp, id: u64) {
        self.log(now, format_args!("uprk[{}]", id));
    }

    fn connect(&mut self, now: Timestamp, connection_id: u64, time: Duration) {
        self.log(
            now,
            format_args!("conn[{}]={:?}us", connection_id, time.as_micros()),
        );
    }
}

#[derive(Clone, Debug, Default)]
pub struct Throughput(Arc<ThroughputInner>);

impl Throughput {
    pub fn reporter(&self, freq: Duration) {
        let handle = self.clone();
        tokio::spawn(async move {
            let start = tokio::time::Instant::now();
            let mut prev = start;
            while !handle.0.is_open.fetch_or(false, Ordering::Relaxed) {
                tokio::time::sleep(freq).await;
                prev = handle.report(start, prev);
            }
            handle.report(start, prev);
        });
    }

    fn report(
        &self,
        start: tokio::time::Instant,
        prev: tokio::time::Instant,
    ) -> tokio::time::Instant {
        let now = tokio::time::Instant::now();
        let elapsed = now - prev;
        let ts = unsafe { Timestamp::from_duration(now - start) };
        let v = self.0.results.take();
        eprintln!("{} {}", ts, v / elapsed);
        now
    }
}

#[derive(Debug, Default)]
struct ThroughputInner {
    results: ThroughputResults,
    is_open: Arc<AtomicBool>,
}

impl Trace for Throughput {
    fn send(&mut self, _now: Timestamp, _stream_id: u64, len: u64) {
        self.0.results.tx.fetch_add(len, Ordering::Relaxed);
    }

    fn receive(&mut self, _now: Timestamp, _stream_id: u64, len: u64) {
        self.0.results.rx.fetch_add(len, Ordering::Relaxed);
    }
}

impl Drop for ThroughputInner {
    fn drop(&mut self) {
        self.is_open.store(false, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Default)]
pub struct ThroughputResults<Counter = Arc<AtomicU64>> {
    rx: Counter,
    tx: Counter,
}

impl ThroughputResults {
    pub fn take(&self) -> ThroughputResults<Byte> {
        ThroughputResults {
            rx: self.rx.swap(0, Ordering::Relaxed).bytes(),
            tx: self.tx.swap(0, Ordering::Relaxed).bytes(),
        }
    }
}

impl core::ops::Div<Duration> for ThroughputResults<Byte> {
    type Output = ThroughputResults<Rate>;

    fn div(self, duration: Duration) -> ThroughputResults<Rate> {
        ThroughputResults {
            rx: self.rx / duration,
            tx: self.tx / duration,
        }
    }
}

impl fmt::Display for ThroughputResults<Rate> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "throughput: rx={:#} tx={:#}", self.rx, self.tx)
    }
}

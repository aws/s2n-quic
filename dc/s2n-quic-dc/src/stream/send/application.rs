// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock,
    event::{self, ConnectionPublisher},
    msg,
    stream::{
        pacer, runtime,
        send::{flow, queue},
        shared::ArcShared,
        socket,
    },
};
use core::{
    fmt,
    pin::Pin,
    sync::atomic::Ordering,
    task::{Context, Poll},
};
use s2n_quic_core::{buffer, ensure, ready, task::waker, time::Timestamp};
use std::{io, net::SocketAddr};
use tracing::trace;

mod builder;
pub mod state;
pub mod transmission;

pub use builder::Builder;

pub struct Writer<Sub: event::Subscriber>(Box<Inner<Sub>>);

struct Inner<Sub>
where
    Sub: event::Subscriber,
{
    shared: ArcShared<Sub>,
    sockets: socket::ArcApplication,
    queue: queue::Queue,
    pacer: pacer::Naive,
    open: bool,
    runtime: runtime::ArcHandle<Sub>,
}

impl<Sub> fmt::Debug for Writer<Sub>
where
    Sub: event::Subscriber,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("Writer");

        for (name, addr) in [
            ("peer_addr", self.peer_addr()),
            ("local_addr", self.local_addr()),
        ] {
            if let Ok(addr) = addr {
                s.field(name, &addr);
            }
        }

        s.finish()
    }
}

impl<Sub> Writer<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.shared.common.ensure_open()?;
        Ok(self.0.shared.write_remote_addr().into())
    }

    #[inline]
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.sockets.write_application().local_addr()
    }

    #[inline]
    pub fn protocol(&self) -> socket::Protocol {
        self.0.sockets.protocol()
    }

    #[inline]
    pub async fn write_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, false)).await
    }

    #[inline]
    pub fn poll_write_from<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        let start_time = self.0.shared.clock.get_time();
        let provided_len = buf.buffered_len();

        let res = waker::debug_assert_contract(cx, |cx| {
            // if we've already shut down the stream then return early
            if !self.0.open {
                ensure!(
                    buf.buffer_is_empty() && is_fin,
                    Err(io::Error::from(io::ErrorKind::BrokenPipe)).into()
                );
                return Ok(0).into();
            }

            let res = ready!(self.0.poll_write_from(cx, buf, is_fin));

            // if we got an error then shut down the stream if needed
            if res.is_err() {
                // use the `Drop` type so we send a RST instead
                let _ = self.0.shutdown(ShutdownType::Drop {
                    is_panicking: false,
                });
            }

            res.into()
        });

        self.0
            .publish_write_events(provided_len, is_fin, start_time, &res);

        res
    }

    /// Shutdown the stream for writing.
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.0.shutdown(ShutdownType::Explicit)
    }
}

impl<Sub> Inner<Sub>
where
    Sub: event::Subscriber,
{
    #[inline(always)]
    fn poll_write_from<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        // Try to flush any pending packets
        let flushed_len = ready!(self.poll_flush_buffer(cx, buf.buffered_len()))?;

        // if the flushed len is non-zero then return it to the application before accepting more
        // bytes to buffer
        ensure!(flushed_len == 0, Ok(flushed_len).into());

        // make sure the queue is drained before continuing
        ensure!(self.queue.is_empty(), Ok(flushed_len).into());

        let app = self.shared.application();
        let max_header_len = app.max_header_len();
        let max_segments = self.shared.gso.max_segments();

        // create a flow request from the provided application input
        let initial_len = buf.buffered_len();
        let mut request = flow::Request {
            len: initial_len,
            initial_len,
            is_fin,
        };

        let path = self.shared.sender.path.load();

        // clamp the flow request based on the path state
        request.clamp(path.max_flow_credits(max_header_len, max_segments));

        // acquire flow credits from the worker
        let credits = ready!(self.shared.sender.flow.poll_acquire(cx, request))?;

        trace!(?credits);

        let features = self.sockets.features();

        let mut batch = if features.is_reliable() {
            // the protocol does recovery for us so no need to track the transmissions
            None
        } else {
            // if we are using unreliable sockets then we need to write transmissions to a batch for the
            // worker to track for recovery

            let batch = self
                .shared
                .sender
                .application_transmission_queue
                .alloc_batch(msg::segment::MAX_COUNT);
            Some(batch)
        };

        self.queue.push_buffer(
            buf,
            &mut batch,
            max_segments,
            &self.shared.sender.segment_alloc,
            |message, buf| {
                self.shared.crypto.seal_with(
                    |sealer| {
                        // push packets for transmission into our queue
                        app.transmit(
                            credits,
                            &path,
                            buf,
                            &self.shared.sender.packet_number,
                            sealer,
                            self.shared.credentials(),
                            &clock::Cached::new(&self.shared.clock),
                            message,
                        )
                    },
                    |sealer| {
                        if features.is_reliable() {
                            sealer.update(&self.shared.clock, &self.shared.subscriber);
                        } else {
                            // TODO enqueue a full flush of any pending transmissions before
                            // updating the key.
                        }
                    },
                )
            },
        )?;

        if let Some(batch) = batch {
            // send the transmission information off to the worker before flushing to the socket so the
            // worker is prepared to handle ACKs from the peer
            self.shared.sender.push_to_worker(batch)?;
        }

        // flush the queue of packets to the socket
        self.poll_flush_buffer(cx, usize::MAX)
    }

    #[inline]
    fn poll_flush_buffer(
        &mut self,
        cx: &mut Context,
        limit: usize,
    ) -> Poll<Result<usize, io::Error>> {
        // if we're actually writing to the socket then we need to pace
        if !self.queue.is_empty() {
            ready!(self.pacer.poll_pacing(cx, &self.shared.clock));
        }

        let len = ready!(self.queue.poll_flush(
            cx,
            limit,
            self.sockets.write_application(),
            &msg::addr::Addr::new(self.shared.write_remote_addr()),
            &self.shared.sender.segment_alloc,
            &self.shared.gso,
            &self.shared.clock,
            &self.shared.subscriber,
        ))?;

        Ok(len).into()
    }

    #[inline]
    fn shutdown(&mut self, ty: ShutdownType) -> io::Result<()> {
        // make sure we haven't already shut down
        ensure!(
            self.open,
            // macos returns an error after the stream has already shut down
            if cfg!(target_os = "macos") {
                Err(io::ErrorKind::NotConnected.into())
            } else {
                Ok(())
            }
        );

        // TODO what do we want to do when we are panicking?
        if !matches!(ty, ShutdownType::Drop { is_panicking: true }) {
            // don't block on this actually completing since we want to also notify the worker
            // immediately
            let waker = s2n_quic_core::task::waker::noop();
            let mut cx = core::task::Context::from_waker(&waker);
            let _ = self.poll_write_from(&mut cx, &mut buffer::reader::storage::Empty, true);
        }

        self.open = false;
        self.shared
            .common
            .closed_halves
            .fetch_add(1, Ordering::Relaxed);

        let queue = core::mem::take(&mut self.queue);

        // if we've transmitted everything we need to then finished the writing half
        if matches!(ty, ShutdownType::Explicit) && queue.is_empty() {
            self.sockets.write_application().send_finish()?;
        }

        let buffer_len = queue.accepted_len();

        // pass things to the worker if we need to gracefully shut down
        if !self.sockets.features().is_stream() {
            self.shared
                .publisher()
                .on_stream_write_shutdown(event::builder::StreamWriteShutdown {
                    background: false,
                    buffer_len,
                });

            let is_panicking = matches!(ty, ShutdownType::Drop { is_panicking: true });
            self.shared.sender.shutdown(queue, is_panicking);
            return Ok(());
        }

        let background = !queue.is_empty();

        self.shared
            .publisher()
            .on_stream_write_shutdown(event::builder::StreamWriteShutdown {
                background,
                buffer_len,
            });

        // if we're using TCP and we get blocked from writing a final offset then spawn a task
        // to do it for us
        if background {
            let shared = self.shared.clone();
            let sockets = self.sockets.clone();
            self.runtime.spawn_send_shutdown(Shutdown {
                queue,
                shared,
                sockets,
                ty,
            });
        }

        Ok(())
    }

    #[inline(always)]
    fn publish_write_events(
        &self,
        provided_len: usize,
        is_fin: bool,
        start_time: Timestamp,
        result: &Poll<io::Result<usize>>,
    ) {
        let end_time = self.shared.clock.get_time();
        let processing_duration = end_time.saturating_duration_since(start_time);

        match result {
            Poll::Ready(Ok(len)) if is_fin => {
                self.shared.common.publisher().on_stream_write_fin_flushed(
                    event::builder::StreamWriteFinFlushed {
                        provided_len,
                        committed_len: *len,
                        processing_duration,
                    },
                );
            }
            Poll::Ready(Ok(len)) => {
                self.shared.common.publisher().on_stream_write_flushed(
                    event::builder::StreamWriteFlushed {
                        provided_len,
                        committed_len: *len,
                        processing_duration,
                    },
                );
            }
            Poll::Ready(Err(error)) => {
                let errno = error.raw_os_error();
                self.shared.common.publisher().on_stream_write_errored(
                    event::builder::StreamWriteErrored {
                        provided_len,
                        is_fin,
                        processing_duration,
                        errno,
                    },
                );
            }
            Poll::Pending => {
                self.shared.common.publisher().on_stream_write_blocked(
                    event::builder::StreamWriteBlocked {
                        provided_len,
                        is_fin,
                        processing_duration,
                    },
                );
            }
        };
    }
}

#[cfg(feature = "tokio")]
impl<Sub> tokio::io::AsyncWrite for Writer<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.poll_write_from(cx, &mut buf, false)
    }

    #[inline]
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[std::io::IoSlice],
    ) -> Poll<Result<usize, io::Error>> {
        let mut buf = buffer::reader::storage::IoSlice::new(buf);
        self.poll_write_from(cx, &mut buf, false)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        // no-op to match TCP semantics
        // https://github.com/tokio-rs/tokio/blob/ee68c1a8c211300ee862cbdd34c48292fa47ac3b/tokio/src/net/tcp/stream.rs#L1358
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        self.0.shutdown(ShutdownType::Explicit).into()
    }

    #[inline(always)]
    fn is_write_vectored(&self) -> bool {
        true
    }
}

impl<Sub> Drop for Writer<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn drop(&mut self) {
        let _ = self.0.shutdown(ShutdownType::Drop {
            is_panicking: std::thread::panicking(),
        });
    }
}

#[derive(Clone, Copy, Debug)]
enum ShutdownType {
    Explicit,
    Drop { is_panicking: bool },
}

pub struct Shutdown<Sub>
where
    Sub: event::Subscriber,
{
    queue: queue::Queue,
    shared: ArcShared<Sub>,
    sockets: socket::ArcApplication,
    ty: ShutdownType,
}

impl<Sub> core::future::Future for Shutdown<Sub>
where
    Sub: event::Subscriber,
{
    type Output = ();

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let Self {
            queue,
            sockets,
            shared,
            ty,
        } = self.get_mut();

        // flush the buffer
        let _ = ready!(queue.poll_flush(
            cx,
            usize::MAX,
            sockets.write_application(),
            &msg::addr::Addr::new(shared.write_remote_addr()),
            &shared.sender.segment_alloc,
            &shared.gso,
            &shared.clock,
            &shared.subscriber,
        ));

        // If the application is explicitly shutting down then do the same. Otherwise let
        // the stream `close` and send a RST
        if matches!(ty, ShutdownType::Explicit) {
            let _ = sockets.write_application().send_finish();
        }

        Poll::Ready(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn shutdown_traits_test<Sub>(shutdown: &Shutdown<Sub>)
    where
        Sub: event::Subscriber,
    {
        use crate::testing::*;

        assert_send(shutdown);
        assert_sync(shutdown);
        assert_static(shutdown);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::Timer,
    event::{self, ConnectionPublisher as _},
    msg,
    stream::{recv, runtime, shared::ArcShared, socket},
};
use core::{
    fmt,
    mem::ManuallyDrop,
    pin::Pin,
    task::{Context, Poll},
};
use s2n_quic_core::{
    buffer::{self, writer::Storage as _},
    ensure, ready,
    stream::state,
    task::waker,
    time::{clock::Timer as _, timer::Provider as _, Timestamp},
};
use std::{io, net::SocketAddr};

mod builder;
pub use builder::Builder;

pub use crate::stream::recv::shared::AckMode;

/// Defines what strategy to use when writing to the provided buffer
#[derive(Clone, Copy, Debug, Default)]
pub enum ReadMode {
    /// Will attempt to read packets from the socket until the application buffer is full
    UntilFull,
    /// Will only attempt to read packets once
    #[default]
    Once,
    /// Will attempt to drain the socket, even if the buffer isn't capable of reading it right now
    Drain,
}

pub struct Reader<Sub: event::Subscriber>(ManuallyDrop<Box<Inner<Sub>>>);

pub(crate) struct Inner<Sub>
where
    Sub: event::Subscriber,
{
    shared: ArcShared<Sub>,
    sockets: socket::ArcApplication,
    send_buffer: msg::send::Message,
    read_mode: ReadMode,
    ack_mode: AckMode,
    timer: Option<Timer>,
    local_state: LocalState,
    runtime: runtime::ArcHandle<Sub>,
}

impl<Sub> fmt::Debug for Reader<Sub>
where
    Sub: event::Subscriber,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = f.debug_struct("Reader");

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

#[derive(Clone, Debug, Default)]
enum LocalState {
    #[default]
    Ready,
    Reading,
    Drained,
    Errored(recv::Error),
}

impl LocalState {
    #[inline]
    fn check(&self) -> Option<io::Result<()>> {
        match self {
            Self::Ready | Self::Reading => None,
            Self::Drained => Some(Ok(())),
            Self::Errored(err) => Some(Err((*err).into())),
        }
    }

    #[inline]
    fn on_read(&mut self) {
        ensure!(matches!(self, Self::Ready));
        *self = Self::Reading;
    }

    #[inline]
    fn transition<Sub>(&mut self, target: Self, shared: &ArcShared<Sub>)
    where
        Sub: event::Subscriber,
    {
        ensure!(matches!(self, Self::Ready | Self::Reading));
        *self = target;

        shared
            .common
            .closed_halves
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

impl<Sub> Reader<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.shared.common.ensure_open()?;
        Ok(self.0.shared.read_remote_addr().into())
    }

    #[inline]
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.sockets.read_application().local_addr()
    }

    #[inline]
    pub fn protocol(&self) -> socket::Protocol {
        self.0.sockets.protocol()
    }

    #[inline]
    pub async fn read_into<S>(&mut self, out_buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        core::future::poll_fn(|cx| self.poll_read_into(cx, out_buf)).await
    }

    #[inline]
    pub fn poll_read_into<S>(
        &mut self,
        cx: &mut Context,
        out_buf: &mut S,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        let start_time = self.0.shared.clock.get_time();
        let capacity = out_buf.remaining_capacity();

        let result = waker::debug_assert_contract(cx, |cx| {
            let mut out_buf = out_buf.track_write();
            let res = self.0.poll_read_into(cx, &mut out_buf);

            if res.is_pending() {
                debug_assert_eq!(
                    out_buf.written_len(),
                    0,
                    "bytes should only be written on Ready(_)"
                );
            }

            let res = ready!(res);
            // record the first time we get `Poll::Ready`
            self.0.local_state.on_read();
            res?;

            Ok(out_buf.written_len()).into()
        });

        self.0.publish_read_events(capacity, start_time, &result);

        result
    }
}

impl<Sub> Inner<Sub>
where
    Sub: event::Subscriber,
{
    #[inline(always)]
    fn poll_read_into<S>(
        &mut self,
        cx: &mut Context,
        out_buf: &mut buffer::writer::storage::Tracked<S>,
    ) -> Poll<io::Result<()>>
    where
        S: buffer::writer::Storage,
    {
        if let Some(res) = self.local_state.check() {
            return res.into();
        }

        // force a read on the socket if the application gave us an empty buffer
        let mut force_recv = !out_buf.has_remaining_capacity();

        let shared = &self.shared;
        let sockets = &self.sockets;
        let transport_features = sockets.features();

        let mut reader = shared.receiver.application_guard(
            self.ack_mode,
            &mut self.send_buffer,
            shared,
            sockets,
        )?;
        let reader = &mut *reader;

        loop {
            // try to process any bytes we have in the recv buffer
            reader.process_recv_buffer(out_buf, shared, transport_features);

            // if we still have remaining capacity in the `out_buf` make sure the reassembler is
            // fully drained
            if cfg!(debug_assertions) && out_buf.has_remaining_capacity() {
                assert!(reader.reassembler.is_empty());
            }

            // make sure we don't have an error
            if let Err(err) = reader.receiver.check_error() {
                self.local_state
                    .transition(LocalState::Errored(err), &self.shared);
                return Err(err.into()).into();
            }

            match reader.receiver.state() {
                state::Receiver::Recv | state::Receiver::SizeKnown => {
                    // we haven't received everything so we still need to read from the socket
                }
                state::Receiver::DataRecvd => {
                    // make sure we have capacity in the buffer before looping back around
                    ensure!(out_buf.has_remaining_capacity(), Ok(()).into());

                    // if we've received everything from the sender then no need to poll
                    // the socket at this point
                    continue;
                }
                // if we've copied the entire buffer into the application then just return
                state::Receiver::DataRead => {
                    self.local_state
                        .transition(LocalState::Drained, &self.shared);
                    break;
                }
                // we already checked for an error above
                state::Receiver::ResetRecvd | state::Receiver::ResetRead => unreachable!(),
            }

            match self.read_mode {
                // ignore the mode if we have a forced receive
                _ if force_recv => {}
                // if we've completely filled the `out_buf` then we're done
                ReadMode::UntilFull if !out_buf.has_remaining_capacity() => break,
                // if we've read at least one byte then we're done
                ReadMode::Once if out_buf.written_len() > 0 => break,
                // otherwise keep going
                _ => {}
            }

            let recv = reader.poll_fill_recv_buffer(
                cx,
                self.sockets.read_application(),
                &self.shared.clock,
                &self.shared.subscriber,
            );

            let recv_len =
                match Self::handle_socket_result(cx, &mut reader.receiver, &mut self.timer, recv) {
                    Poll::Ready(res) => res?,
                    // if we've written at least one byte then return that amount
                    Poll::Pending if out_buf.written_len() > 0 => break,
                    Poll::Pending => return Poll::Pending,
                };

            // clear the forced receive after performing it once
            force_recv = false;

            if recv_len == 0 {
                if transport_features.is_stream() {
                    // if we got a 0-length read then the stream was closed - notify the receiver
                    reader.receiver.on_transport_close();
                    continue;
                } else {
                    debug_assert!(false, "datagram recv buffers should never be empty");
                }
            }
        }

        Ok(()).into()
    }

    #[inline]
    fn handle_socket_result(
        cx: &mut Context,
        receiver: &mut recv::state::State,
        timer: &mut Option<Timer>,
        res: Poll<io::Result<usize>>,
    ) -> Poll<io::Result<usize>> {
        if let Poll::Ready(res) = res {
            return res.into();
        }

        // only check the timer if we have one
        let Some(timer) = timer.as_mut() else {
            return Poll::Pending;
        };

        // if we didn't get any packets then poll the timer
        if let Some(target) = receiver.next_expiration() {
            timer.update(target);
            ready!(timer.poll_ready(cx));

            // if the timer expired then keep going, even if the recv buffer is empty
            // we return `1` to make the caller think that something was written to the buffer
            Ok(1).into()
        } else {
            timer.cancel();
            Poll::Pending
        }
    }

    #[inline]
    fn shutdown(mut self: Box<Self>) {
        // If the application never read from the stream try to do so now
        if let LocalState::Ready = self.local_state {
            let mut storage = buffer::writer::storage::Empty;
            let waker = s2n_quic_core::task::waker::noop();
            let mut cx = core::task::Context::from_waker(&waker);
            let _ = self.poll_read_into(&mut cx, &mut storage.track_write());
        }

        let background = matches!(self.local_state, LocalState::Ready);

        self.shared
            .publisher()
            .on_stream_read_shutdown(event::builder::StreamReadShutdown { background });

        // If we haven't exited the `Ready` state then spawn a task to do it for the application
        //
        // This is important for processing any secret control packets that the server sends us
        if background {
            tracing::debug!("spawning task to read server's response");
            let runtime = self.runtime.clone();
            let handle = Shutdown(self);
            runtime.spawn_recv_shutdown(handle);
            return;
        }

        // update the common closed state if needed
        self.local_state
            .transition(LocalState::Drained, &self.shared);

        // let the peer know if we shut down cleanly
        let is_panicking = std::thread::panicking();

        self.shared.receiver.shutdown(is_panicking);
    }

    #[inline(always)]
    fn publish_read_events(
        &self,
        capacity: usize,
        start_time: Timestamp,
        result: &Poll<io::Result<usize>>,
    ) {
        let end_time = self.shared.clock.get_time();
        let processing_duration = end_time.saturating_duration_since(start_time);

        match result {
            Poll::Ready(Ok(0)) if capacity > 0 => {
                self.shared.common.publisher().on_stream_read_fin_flushed(
                    event::builder::StreamReadFinFlushed {
                        capacity,
                        processing_duration,
                    },
                );
            }
            Poll::Ready(Ok(len)) => {
                self.shared.common.publisher().on_stream_read_flushed(
                    event::builder::StreamReadFlushed {
                        capacity,
                        committed_len: *len,
                        processing_duration,
                    },
                );
            }
            Poll::Ready(Err(error)) => {
                let errno = error.raw_os_error();
                self.shared.common.publisher().on_stream_read_errored(
                    event::builder::StreamReadErrored {
                        capacity,
                        processing_duration,
                        errno,
                    },
                );
            }
            Poll::Pending => {
                self.shared.common.publisher().on_stream_read_blocked(
                    event::builder::StreamReadBlocked {
                        capacity,
                        processing_duration,
                    },
                );
            }
        };
    }
}

#[cfg(feature = "tokio")]
impl<Sub> tokio::io::AsyncRead for Reader<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut buf = buffer::writer::storage::BufMut::new(buf);
        ready!(self.poll_read_into(cx, &mut buf))?;
        Ok(()).into()
    }
}

impl<Sub> Drop for Reader<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn drop(&mut self) {
        let inner = unsafe {
            // SAFETY: the inner type is only taken once
            ManuallyDrop::take(&mut self.0)
        };
        inner.shutdown();
    }
}

pub struct Shutdown<Sub: event::Subscriber>(Box<Inner<Sub>>);

impl<Sub> core::future::Future for Shutdown<Sub>
where
    Sub: event::Subscriber,
{
    type Output = ();

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let mut storage = buffer::writer::storage::Empty;
        let _ = ready!(self.0.poll_read_into(cx, &mut storage.track_write()));
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

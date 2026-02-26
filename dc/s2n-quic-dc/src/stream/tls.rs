// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides support code for TLS streams.
//!
//! TLS is integrated into dcQUIC streams via s2n-tls, with two primary phases:
//!
//! * Handshaking (`poll_negotiate`) is handled with raw socket operations, i.e., s2n-tls directly
//!   reads/writes from the underlying socket.
//! * Dataplane I/O (`poll_send` / `poll_recv`) are handled via s2n-quic-dc owned buffers. s2n-tls
//!   operations read/write from in-memory buffers that are filled / emptied by s2n-quic-dc. This
//!   means that sending/receiving doesn't need to setup async state in s2n-tls since all
//!   operations finish synchronously. (EWOULDBLOCK is still needed when reading, but registering
//!   read interest and refilling the buffer is handled by wrapping s2n-quic-dc code).
//!
//! A future revision is expected to replace the dataplane I/O with a non-s2n-tls backed
//! implementation that will reduce intermediate buffering / copies that the current strategy
//! forces. This will also eliminate the Mutex wrapping the s2n-tls connection.

use s2n_quic_core::{
    buffer::{reader::Incremental, writer::Storage as _, Writer as _},
    time::Timestamp,
    varint::VarInt,
};
use std::{
    io,
    sync::{Arc, Mutex},
    task::Poll,
    time::Duration,
};
use tokio::net::TcpStream;

use crate::stream::socket::{application::Single, Application};

pub struct S2nTlsConnection {
    socket: Arc<Single<TcpStream>>,
    connection: Mutex<(s2n_tls::connection::Connection, ReadState)>,
}

struct ReadState {
    reader: Incremental,
    buffer: bytes::BytesMut,
}

impl S2nTlsConnection {
    pub fn from_connection(
        socket: Arc<Single<TcpStream>>,
        mut connection: s2n_tls::connection::Connection,
    ) -> io::Result<Self> {
        connection.set_blinding(s2n_tls::enums::Blinding::SelfService)?;

        Ok(S2nTlsConnection {
            socket,
            connection: Mutex::new((
                connection,
                ReadState {
                    reader: Incremental::new(VarInt::ZERO),
                    buffer: bytes::BytesMut::with_capacity(8192),
                },
            )),
        })
    }

    pub(crate) async fn negotiate(&mut self) -> io::Result<()> {
        std::future::poll_fn(|cx| -> Poll<io::Result<()>> {
            let connection = &mut self.connection.get_mut().unwrap().0;

            let context = NegotiateContext {
                socket: &self.socket,
                waker: cx.waker(),
            };

            let mut connection = CallbackResetGuard {
                conn: connection,
                reset_write: true,
                reset_read: true,
            };

            connection.set_receive_callback(Some(recv_direct_cb))?;
            connection.set_send_callback(Some(send_direct_cb))?;

            let mut connection = connection.set_context(&context);

            let res = match connection.poll_negotiate() {
                Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
                Poll::Pending => Poll::Pending,
            };

            drop(connection);

            res
        })
        .await
    }

    pub(crate) fn write<M, R>(
        &self,
        message: &mut M,
        reader: &mut R,
        is_fin: bool,
    ) -> Result<(), crate::stream::send::Error>
    where
        M: super::send::application::state::Message,
        R: s2n_quic_core::buffer::reader::storage::Infallible,
    {
        let mut guard = self.connection.lock().unwrap();
        let conn = CallbackResetGuard {
            conn: &mut guard.0,
            reset_write: true,
            reset_read: false,
        };

        let mut conn = conn.set_context_mut(message);
        conn.set_send_callback(Some(send_io_cb::<M>))
            .expect("infallible");

        // FIXME: If the application writes a large payload, this loop ends up buffering that
        // payload inside the s2n-quic-dc buffer before we even start transmitting to the network.
        // This ends up using more memory than strictly needed, and likely increasing end-to-end
        // latency. We should consider limiting how much encrypted data we are willing to buffer.
        while !reader.buffer_is_empty() {
            let Ok(chunk) = reader.read_chunk(usize::MAX);
            let mut consumed = 0;
            while consumed < chunk.len() {
                match conn.poll_send(&chunk) {
                    Poll::Ready(Ok(l)) => consumed += l,
                    Poll::Ready(Err(e)) => {
                        tracing::warn!("s2n_tls::poll_send() = Err({:?})", &e);
                        return Err(crate::stream::send::Error::new(
                            crate::stream::send::ErrorKind::FatalError,
                        ));
                    }
                    Poll::Pending => unreachable!(
                        "TODO: verify, but s2n-tls shouldn't block when the network doesn't"
                    ),
                }
            }
        }

        if is_fin {
            match conn.poll_shutdown_send() {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(e)) => {
                    tracing::warn!("s2n_tls::poll_shutdown_send() = Err({:?})", &e);
                    return Err(crate::stream::send::Error::new(
                        crate::stream::send::ErrorKind::FatalError,
                    ));
                }
                Poll::Pending => unreachable!(
                    "TODO: verify, but s2n-tls shouldn't block when the network doesn't"
                ),
            }
        }

        Ok(())
    }

    /// Process TLS frames in `input` and write decrypted results into `output`.
    pub(crate) fn read(
        &self,
        input: &mut super::recv::shared::RecvBuffer,
        output: &mut s2n_quic_core::buffer::duplex::Interposer<
            '_,
            impl s2n_quic_core::buffer::writer::Storage,
            s2n_quic_core::buffer::Reassembler,
        >,
    ) -> Result<(), super::recv::Error> {
        let mut guard = self.connection.lock().unwrap();
        let (conn, read_state) = &mut *guard;
        let conn = CallbackResetGuard {
            conn,
            reset_write: false,
            reset_read: true,
        };

        let mut conn = conn.set_context_mut(input);
        conn.set_receive_callback(Some(recv_io_cb)).unwrap();

        // FIXME: We should be reading directly into `output`, but currently Interposer doesn't
        // expose the spare capacity as a buffer we can write to. That's probably fixable since we
        // have some bound on the size
        // (https://docs.rs/s2n-tls/latest/s2n_tls/connection/struct.Connection.html#method.peek_len)
        // but for now this works if a bit less efficiently than we'd like.
        read_state.buffer.reserve(8192);
        match conn.poll_recv_uninitialized(read_state.buffer.spare_capacity_mut()) {
            Poll::Ready(Ok(len)) => {
                // SAFETY: s2n-tls just informed us it filled the buffer by `len` bytes.
                unsafe {
                    let original = read_state.buffer.len();
                    read_state.buffer.set_len(
                        original
                            .checked_add(len)
                            .expect("single buffer cannot exceed isize::MAX, so cannot overflow"),
                    );
                }

                let is_fin = len == 0;

                let mut reader = match read_state
                    .reader
                    .with_storage(&mut read_state.buffer, is_fin)
                {
                    Ok(r) => r,
                    Err(s2n_quic_core::buffer::Error::OutOfRange) => {
                        return Err(super::recv::Error::new(
                            super::recv::ErrorKind::MaxDataExceeded,
                        ))
                    }
                    Err(s2n_quic_core::buffer::Error::InvalidFin) => {
                        return Err(super::recv::Error::new(super::recv::ErrorKind::InvalidFin))
                    }
                };

                match output.read_from(&mut reader) {
                    Ok(()) => {}
                    Err(s2n_quic_core::buffer::Error::OutOfRange) => {
                        return Err(super::recv::Error::new(
                            super::recv::ErrorKind::MaxDataExceeded,
                        ))
                    }
                    Err(s2n_quic_core::buffer::Error::InvalidFin) => {
                        return Err(super::recv::Error::new(super::recv::ErrorKind::InvalidFin))
                    }
                }
            }
            Poll::Ready(Err(e)) => {
                tracing::warn!("s2n_tls::poll_recv() = Err({:?})", &e);
                return Err(super::recv::Error::new(super::recv::ErrorKind::Decode));
            }
            Poll::Pending => {
                // Fall through, we expect to hit this case if we've consumed from recv::Buffer but
                // didn't get enough data to return any to the application.
            }
        }

        Ok(())
    }
}

/// `NegotiateContext` for poll_negotiate.
///
/// This is registered with s2n-tls during poll_negotiate for the callbacks to call, used with
/// [`recv_direct_cb`] and [`send_direct_cb`].
struct NegotiateContext<'a> {
    socket: &'a Single<TcpStream>,
    waker: &'a std::task::Waker,
}

/// The function should return the number of bytes received, or set errno and return an error code < 0.
#[allow(clippy::extra_unused_lifetimes)]
unsafe extern "C" fn recv_direct_cb<'a>(
    ctx: *mut core::ffi::c_void,
    buf: *mut u8,
    len: u32,
) -> i32 {
    let ctx = ctx.cast::<NegotiateContext<'a>>().as_ref::<'a>().unwrap();

    let mut cx = std::task::Context::from_waker(ctx.waker);

    // FIXME: The output is not necessarily initialized, but we don't currently have an
    // uninit-compatible socket read API. In practice the buffer isn't read from but this is
    // potential undefined behavior.
    let buf = std::slice::from_raw_parts_mut(buf, len as usize);
    let buf = std::io::IoSliceMut::new(buf);

    let mut addr = Default::default();
    let mut cmsg = Default::default();

    match ctx
        .socket
        .read_application()
        .poll_recv(&mut cx, &mut addr, &mut cmsg, &mut [buf])
    {
        Poll::Ready(Ok(r)) => r as i32,
        Poll::Ready(Err(e)) => {
            nix::errno::Errno::try_from(e)
                .unwrap_or(nix::errno::Errno::EIO)
                .set();
            -1
        }
        Poll::Pending => {
            nix::errno::Errno::EWOULDBLOCK.set();
            -1
        }
    }
}

/// The function should return the number of bytes sent or set errno and return an error code < 0.
#[allow(clippy::extra_unused_lifetimes)]
unsafe extern "C" fn send_direct_cb<'a>(
    ctx: *mut core::ffi::c_void,
    buf: *const u8,
    len: u32,
) -> i32 {
    let ctx = ctx.cast::<NegotiateContext<'a>>().as_ref::<'a>().unwrap();

    let mut cx = std::task::Context::from_waker(ctx.waker);

    let buf = std::slice::from_raw_parts(buf, len as usize);
    let buf = std::io::IoSlice::new(buf);

    let addr = Default::default();
    let ecn = Default::default();

    match ctx
        .socket
        .write_application()
        .poll_send(&mut cx, &addr, ecn, &[buf])
    {
        Poll::Ready(Ok(r)) => r as i32,
        Poll::Ready(Err(e)) => {
            nix::errno::Errno::try_from(e)
                .unwrap_or(nix::errno::Errno::EIO)
                .set();
            -1
        }
        Poll::Pending => {
            nix::errno::Errno::EWOULDBLOCK.set();
            -1
        }
    }
}

/// The function should return the number of bytes sent or set errno and return an error code < 0.
unsafe extern "C" fn send_io_cb<'a, M>(ctx: *mut core::ffi::c_void, buf: *const u8, len: u32) -> i32
where
    M: 'a + super::send::application::state::Message,
{
    let message = ctx.cast::<M>().as_mut::<'a>().unwrap();

    let mut buf = std::slice::from_raw_parts(buf, len as usize);

    while !buf.is_empty() {
        let part = buf
            .split_off(..buf.len().clamp(0, u16::MAX as usize))
            .unwrap();
        // FIXME: this return whether it allocated or not, we should have the event for that here too.
        message.push(part.len(), |mut b| {
            b.put_slice(part);

            crate::stream::send::application::transmission::Event {
                packet_number: VarInt::ZERO,
                info: crate::stream::send::application::transmission::Info {
                    // Soundness critical to get this right - it's used to set the segment length we
                    // wrote to the socket.
                    packet_len: part.len() as u16,
                    retransmission: None,
                    stream_offset: VarInt::ZERO,
                    payload_len: 0,
                    included_fin: Default::default(),
                    time_sent: unsafe { Timestamp::from_duration(Duration::from_millis(1)) },
                    ecn: Default::default(),
                },
                has_more_app_data: false,
            }
        });
    }

    i32::try_from(len).unwrap()
}

/// The function should return the number of bytes received, or set errno and return an error code < 0.
#[allow(clippy::extra_unused_lifetimes)]
unsafe extern "C" fn recv_io_cb<'a>(ctx: *mut core::ffi::c_void, buf: *mut u8, len: u32) -> i32 {
    // Note that we intentionally aren't assuming unique access, since we intend to call from
    // multiple threads.
    let mut ctx = ctx
        .cast::<super::recv::shared::RecvBuffer>()
        .as_mut::<'a>()
        .unwrap();

    let crate::either::Either::A(a) = &mut ctx else {
        unreachable!("only local buffer for TLS stream");
    };

    let output = bytes::buf::UninitSlice::from_raw_parts_mut(buf, len as usize);
    let written = a.copy_into(output);
    if written == 0 {
        if a.saw_fin() {
            0
        } else {
            nix::errno::Errno::EWOULDBLOCK.set();
            -1
        }
    } else {
        written as i32
    }
}

unsafe extern "C" fn unreachable_recv_io_cb(_: *mut core::ffi::c_void, _: *mut u8, _: u32) -> i32 {
    unreachable!(
        "s2n-tls should not call I/O callbacks outside of application controlled send/receive"
    );
}

unsafe extern "C" fn unreachable_send_io_cb(
    _: *mut core::ffi::c_void,
    _: *const u8,
    _: u32,
) -> i32 {
    unreachable!(
        "s2n-tls should not call I/O callbacks outside of application controlled send/receive"
    );
}

struct CallbackResetGuard<'a> {
    conn: &'a mut s2n_tls::connection::Connection,
    reset_write: bool,
    reset_read: bool,
}

// These setters ensure that we capture the given reference for the duration of
// `CallbackResetGuard`, ensuring it can't get dropped earlier. In the mutable case, it also can't
// be accessed at all.
impl<'a> CallbackResetGuard<'a> {
    fn set_context<T>(self, context: &'a T) -> Self {
        // SAFETY: These are reset in Drop, and we ensure that context lives at least that long by
        // capturing it for the same lifetime as connection.
        //
        // This also relies on a module-wide invariant that the T here is the same as used in callbacks
        // set in surrounding code.
        unsafe {
            if self.reset_write {
                self.conn
                    .set_send_context(context as *const T as *mut std::ffi::c_void)
                    .expect("infallible");
            }
            if self.reset_read {
                self.conn
                    .set_receive_context(context as *const T as *mut std::ffi::c_void)
                    .expect("infallible");
            }
            self
        }
    }

    fn set_context_mut<T>(self, context: &'a mut T) -> Self {
        // SAFETY: These are reset in Drop, and we ensure that context lives at least that long by
        // capturing it for the same lifetime as connection.
        //
        // This also relies on a module-wide invariant that the T here is the same as used in callbacks
        // set in surrounding code.
        unsafe {
            if self.reset_write {
                self.conn
                    .set_send_context(context as *mut _ as *mut std::ffi::c_void)
                    .expect("infallible");
            }
            if self.reset_read {
                self.conn
                    .set_receive_context(context as *mut _ as *mut std::ffi::c_void)
                    .expect("infallible");
            }
            self
        }
    }
}

impl std::ops::Deref for CallbackResetGuard<'_> {
    type Target = s2n_tls::connection::Connection;

    fn deref(&self) -> &Self::Target {
        self.conn
    }
}

impl std::ops::DerefMut for CallbackResetGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
    }
}

impl Drop for CallbackResetGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: Resetting the callbacks is effectively infallible in Rust, they are only doing
        // more than a field write if managed send I/O is enabled in s2n-tls (which it never is for
        // s2n-tls Rust bindings). We need to reset them for soundness so our only option is to
        // abort if we got that wrong.
        //
        // If we panicked then the unwind could run destructors invoking the old callbacks, which
        // would potentially reference already freed memory: the context is pointing to the stack.
        unsafe {
            if self.reset_write {
                self.conn
                    .set_send_context(std::ptr::null_mut())
                    .unwrap_or_else(|_| std::process::abort());
                self.conn
                    .set_send_callback(Some(unreachable_send_io_cb))
                    .unwrap_or_else(|_| std::process::abort());
            }
            if self.reset_read {
                self.conn
                    .set_receive_context(std::ptr::null_mut())
                    .unwrap_or_else(|_| std::process::abort());
                self.conn
                    .set_receive_callback(Some(unreachable_recv_io_cb))
                    .unwrap_or_else(|_| std::process::abort());
            }
        }
    }
}

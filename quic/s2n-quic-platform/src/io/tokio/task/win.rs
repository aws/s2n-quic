// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Socket tasks that bridge the `s2n-quic-platform` message ring buffers to the
//! Windows `WSASendMsg` (USO) / `WSARecvMsg` (URO) syscalls.
//!
//! These mirror the shape of `tokio-io`'s unix task module: `rx`/`tx` async
//! entry points wrap a `tokio::net::UdpSocket` and drive the platform's
//! `Receiver`/`Sender`, which call our [`rx::Socket`]/[`tx::Socket`]
//! implementations over slices of ring [`Message`] entries.
//!
//! Like the unix path, the platform `TxQueue` builds the offload batches: it
//! writes multiple packets into a single ring entry payload and records the
//! segment size on the entry. [`TxSocket::send`] then issues one `WSASendMsg`
//! per entry, adding a `UDP_SEND_MSG_SIZE` control message when the entry holds
//! more than one segment. This is the USO acceleration that lets a single flow
//! exceed the per-syscall packet rate.
//!
//! # Relationship to the unix tasks
//!
//! The overall pipeline is identical on both platforms — ring buffer of
//! `Message` entries, a `rx::Receiver`/`tx::Sender` driver, a `Cooldown` to
//! amortize reactor round-trips, and a `stats::Sender` for telemetry — but the
//! syscall layer underneath differs in a few important ways:
//!
//! * **Batching.** Linux has `sendmmsg`/`recvmmsg`, so `syscall::mmsg` hands the
//!   kernel the whole `entries` slice in one call and the message type *is* the
//!   syscall struct (`libc::mmsghdr`). Winsock has no vectored-datagram
//!   equivalent: `WSASendMsg`/`WSARecvMsg` handle exactly one datagram (or one
//!   USO/URO "super datagram") per call, so the loops below iterate `entries`
//!   and issue one syscall per entry. Offload is therefore the *only* way to
//!   amortize syscall overhead on Windows, which is why USO/URO are enabled
//!   aggressively.
//! * **Message representation.** `syscall::msg`/`mmsg` keep a `msghdr` (with its
//!   `iovec` and control buffer) resident in the ring so the syscall can be
//!   issued directly against ring memory. `WSAMSG` is not stored in the ring;
//!   it is a cheap stack temporary built per call from the entry's payload
//!   pointer plus the scratch `name`/`control` buffers owned by the socket
//!   below. See [`crate::message::wsamsg`] for why.
//! * **Readiness.** The unix tasks wrap a raw fd in `AsyncFd` and speculatively
//!   issue the syscall, only touching readiness after a `WouldBlock`. Here the
//!   socket is a `tokio::net::UdpSocket` and each syscall goes through
//!   `try_io`, which is required for correctness on Windows: tokio's IOCP-based
//!   reactor only re-arms the readiness event when the I/O operation is
//!   performed inside `try_io`, so bypassing it can wedge the task.
//! * **Extension functions.** `WSARecvMsg` has no stable export in `ws2_32.dll`
//!   and must be looked up per socket via `WSAIoctl`; see
//!   [`load_wsarecvmsg`]. `WSASendMsg` *is* exported, so only the receive side
//!   carries an `Option<fn>` and a fallback path.

use crate::{
    features::Gso,
    message::{wsamsg::Message, Message as _},
    socket::{
        ring, stats,
        task::{rx, tx},
    },
    syscall::{
        win::{
            configure_ecn, load_wsarecvmsg, parse_coalesced, parse_ecn, parse_pktinfo,
            recv_message, send_message, ControlBuffer, EcnSupport, RecvMeta, SockAddrBuf,
            WsaRecvMsgFn,
        },
        SocketEvents as _,
    },
};
use core::task::{Context, Poll};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    task::cooldown::Cooldown,
};
use std::{io, os::windows::io::AsRawSocket};
use tokio::{io::Interest, net::UdpSocket};
use windows_sys::Win32::Networking::WinSock::{SOCKET, WSAEMSGSIZE};

/// Spawns the receive loop: drains `WSARecvMsg` (or `recv_from`) into `producer`.
///
/// `socket_low` is the lower-priority socket in the dual-socket ("prioritized")
/// configuration and is drained only after `socket`, exactly as on unix.
pub async fn rx<S: Into<std::net::UdpSocket>>(
    socket: S,
    socket_low: Option<S>,
    producer: ring::Producer<Message>,
    cooldown: Cooldown,
    stats: stats::Sender,
    wsa_enabled: bool,
) -> io::Result<()> {
    let socket = RxSocket::new(socket, wsa_enabled)?;
    let socket_low = match socket_low {
        Some(low) => Some(RxSocket::new(low, wsa_enabled)?),
        None => None,
    };

    let result = rx::Receiver::new(producer, socket, socket_low, cooldown, stats).await;
    match result {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Spawns the transmit loop: flushes `consumer` to the socket via `WSASendMsg`.
///
/// `gso` is the shared segment-count handle: `tx::Sender` reads it to decide how
/// many packets to pack into one entry, and disables it if a send fails with an
/// offload error (see `features::gso`).
pub async fn tx<S: Into<std::net::UdpSocket>>(
    socket: S,
    consumer: ring::Consumer<Message>,
    gso: Gso,
    cooldown: Cooldown,
    stats: stats::Sender,
    wsa_enabled: bool,
) -> io::Result<()> {
    let socket = TxSocket::new(socket, wsa_enabled)?;
    let result = tx::Sender::new(consumer, socket, gso, cooldown, stats).await;
    match result {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Hands the socket to tokio.
///
/// The socket must be non-blocking before registration: Winsock has no
/// `O_NONBLOCK`-style flag on the handle itself, it is a per-socket mode set via
/// `ioctlsocket(FIONBIO)`, which is what `set_nonblocking` calls. Without it the
/// `try_io` calls below would block the runtime worker thread.
fn into_async(socket: impl Into<std::net::UdpSocket>) -> io::Result<UdpSocket> {
    let socket = socket.into();
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket)
}

/// Receive side: wraps a socket with the `WSARecvMsg` extension pointer and the
/// scratch name/control buffers required by the syscall.
///
/// URO coalescing and ECN reception are configured by `syscall::configure_gro`
/// and `syscall::configure_tos` before the socket is handed to this task, which
/// keeps the receive buffer sizing in `io::tokio` consistent with the maximum
/// coalesced size accepted by the stack.
///
/// The `name` and `control` buffers are owned by the socket rather than by each
/// ring entry. On unix the equivalent storage lives inline in the ring's
/// `msghdr` entries so `recvmmsg` can fill many at once; since only one
/// `WSARecvMsg` is ever in flight per task, a single reusable scratch pair is
/// enough and keeps the ring entries small (payload pointer + metadata).
pub struct RxSocket {
    socket: UdpSocket,
    recvmsg: Option<WsaRecvMsgFn>,
    name: SockAddrBuf,
    control: ControlBuffer,
}

impl RxSocket {
    fn new<S: Into<std::net::UdpSocket>>(socket: S, wsa_enabled: bool) -> io::Result<Self> {
        let socket = into_async(socket)?;
        let recvmsg = if wsa_enabled {
            load_wsarecvmsg(socket.as_raw_socket() as SOCKET)
        } else {
            None
        };

        Ok(Self {
            socket,
            recvmsg,
            name: SockAddrBuf::default(),
            control: ControlBuffer::default(),
        })
    }

    /// Receives one (possibly URO-coalesced) message into `entry` via
    /// `WSARecvMsg`, decoding the coalesced segment size, ECN marking and the
    /// local address the datagram arrived on.
    ///
    /// A single call can return many datagrams' worth of bytes: with URO enabled
    /// the stack concatenates consecutive datagrams from the same 4-tuple that
    /// all have the same size, and reports that size in the
    /// `UDP_COALESCED_INFO` control message. This is the Windows analogue of
    /// Linux `UDP_GRO`. The trailing datagram may be shorter than the segment
    /// size (it terminates the coalesced run), which is why the split is done by
    /// `RxMessage::for_each` rather than assuming an exact multiple.
    ///
    /// When the control message is absent the datagram was not coalesced, so the
    /// segment size falls back to the total length, i.e. "one segment".
    fn recv_coalesced(
        &mut self,
        recvmsg: WsaRecvMsgFn,
        raw: SOCKET,
        entry: &mut Message,
    ) -> io::Result<()> {
        let name = &mut self.name;
        let control = &mut self.control;
        let meta: RecvMeta = self.socket.try_io(Interest::READABLE, || {
            recv_message(recvmsg, raw, entry.payload_mut(), name, control)
        })?;

        let n = meta.len;
        // `recv_message` already recorded how many control bytes Winsock wrote,
        // so this slice only covers messages from this call
        let control = control.as_slice();
        let seg = parse_coalesced(control)
            .map(|s| s as usize)
            .filter(|&s| s > 0)
            .unwrap_or(n);
        let ecn = ExplicitCongestionNotification::new(parse_ecn(control));
        let local = parse_pktinfo(control);

        entry.set_recv(meta.remote, local, ecn, n, seg);
        Ok(())
    }
}

impl rx::Socket<Message> for RxSocket {
    type Error = io::Error;

    #[inline]
    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [Message],
        events: &mut rx::Events,
        stats: &stats::Sender,
    ) -> io::Result<()> {
        // `SOCKET` is a pointer-sized handle on Windows, unlike the `c_int` fd
        // on unix, so it is widened rather than truncated here
        let raw = self.socket.as_raw_socket() as SOCKET;
        let mut idx = 0;

        loop {
            while idx < entries.len() {
                let entry = &mut entries[idx];
                let res = match self.recvmsg {
                    Some(recvmsg) => self.recv_coalesced(recvmsg, raw, entry),
                    None => self.recv_fallback(entry),
                };

                match res {
                    Ok(()) => {
                        stats.recv().on_operation_ready(1);
                        idx += 1;
                        if events.on_complete(1).is_break() {
                            return Ok(());
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        // Do not advance `idx` on error: the slot is left unfilled
                        // rather than released to the endpoint with stale contents.
                        // UDP recv errors (e.g. WSAECONNRESET) are per-datagram and
                        // clear once read, so the entry is retried later.
                        if let Some(code) = e.raw_os_error() {
                            stats.recv().on_error(code);
                        }

                        if events.on_error(e).is_break() {
                            // Mark the task blocked so the caller stops draining:
                            // the socket is likely still readable and retrying the
                            // same failing call in place would spin. Readiness only
                            // registers a waker while pending, so an immediate wake
                            // is scheduled otherwise to avoid stalling the task.
                            events.blocked();
                            if self.socket.poll_recv_ready(cx).is_ready() {
                                cx.waker().wake_by_ref();
                            }
                            return Ok(());
                        }

                        // otherwise skip the entry and continue with the rest
                        idx += 1;
                    }
                }
            }

            if idx >= entries.len() {
                return Ok(());
            }

            // The socket blocked; re-arm readiness (which also registers the
            // waker). If it is immediately ready again, keep draining.
            match self.socket.poll_recv_ready(cx) {
                Poll::Ready(Ok(())) => continue,
                Poll::Ready(Err(e)) => return Err(e),
                Poll::Pending => {
                    stats.recv().on_operation_pending();
                    events.blocked();
                    return Ok(());
                }
            }
        }
    }
}

impl RxSocket {
    /// Fallback receive used when the Winsock provider doesn't expose the
    /// `WSARecvMsg` extension: a plain `recv_from` with no ECN/URO metadata.
    fn recv_fallback(&mut self, entry: &mut Message) -> io::Result<()> {
        let (n, from) = self.socket.try_recv_from(entry.payload_mut())?;
        let remote: SocketAddress = from.into();
        entry.set_recv(
            remote,
            None,
            ExplicitCongestionNotification::default(),
            n,
            n,
        );
        Ok(())
    }
}

/// Transmit side: sends each ring entry with a single `WSASendMsg`, using USO
/// when the entry holds multiple segments.
///
/// Unlike the unix side there is no `sendmmsg` fast path, so throughput depends
/// on USO: `tx::Sender` fills each entry with up to `Gso::max_segments()`
/// packets and this issues one syscall for the whole batch.
pub struct TxSocket {
    socket: UdpSocket,
    /// Which ECN levels the stack accepted. Sending an ECN control message that
    /// the provider doesn't implement fails the whole datagram, so it is only
    /// emitted for supported levels.
    ecn_support: EcnSupport,
    /// When `false`, datagrams are sent with plain `send_to` instead of `WSASendMsg`.
    wsa_enabled: bool,
}

impl TxSocket {
    fn new<S: Into<std::net::UdpSocket>>(socket: S, wsa_enabled: bool) -> io::Result<Self> {
        let socket = into_async(socket)?;
        let ecn_support = configure_ecn(socket.as_raw_socket() as SOCKET);

        Ok(Self {
            socket,
            ecn_support,
            wsa_enabled,
        })
    }
}

impl tx::Socket<Message> for TxSocket {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        cx: &mut Context,
        entries: &mut [Message],
        events: &mut tx::Events,
        stats: &stats::Sender,
    ) -> io::Result<()> {
        let raw = self.socket.as_raw_socket() as SOCKET;
        let ecn_support = self.ecn_support;
        let mut idx = 0;

        loop {
            while idx < entries.len() {
                let entry = &entries[idx];
                let dest = *entry.remote_address();
                let ecn = entry.ecn();
                // the platform `TxQueue` records a segment size only when the
                // payload holds more than one datagram
                let segment_size = entry.segment_size();
                let payload = entry.payload();

                // `try_io` both performs the send and tells tokio's IOCP
                // reactor whether readiness should be cleared; the send must not
                // be issued outside of it. `try_send_to` does the same
                // internally.
                let res = if self.wsa_enabled {
                    self.socket.try_io(Interest::WRITABLE, || {
                        send_message(raw, payload, segment_size, ecn, &dest, ecn_support)
                    })
                } else {
                    self.socket.try_send_to(payload, dest.into()).map(|_| ())
                };

                match res {
                    Ok(()) => {
                        stats.send().on_operation_ready(1);
                        idx += 1;
                        if events.on_complete(1).is_break() {
                            return Ok(());
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        if let Some(code) = e.raw_os_error() {
                            stats.send().on_error(code);
                        }

                        idx += 1;

                        // `WSAEMSGSIZE` is ambiguous once the DF bit is set by
                        // `syscall::configure_mtu_disc`: it means either that the
                        // provider rejected a coalesced (USO) buffer, or that the
                        // datagram exceeded the MTU the stack enforces (the
                        // interface MTU under `IP_PMTUDISC_PROBE`, the cached path
                        // MTU when it fell back to plain DF). Only the former
                        // should disable segmentation offload, so a single-segment
                        // send that fails this way - typically a DPLPMTUD probe -
                        // is discarded like a lost packet instead of being
                        // reported to the `Gso` handle.
                        let is_pmtu_error =
                            segment_size.is_none() && e.raw_os_error() == Some(WSAEMSGSIZE);

                        let flow = if is_pmtu_error {
                            events.on_complete(1)
                        } else {
                            // the entry is discarded and the error is reported to
                            // the `Gso` handle, which may disable segmentation
                            // offload
                            events.on_error(e)
                        };

                        if flow.is_break() {
                            return Ok(());
                        }
                    }
                }
            }

            if idx >= entries.len() {
                return Ok(());
            }

            match self.socket.poll_send_ready(cx) {
                Poll::Ready(Ok(())) => continue,
                Poll::Ready(Err(e)) => return Err(e),
                Poll::Pending => {
                    stats.send().on_operation_pending();
                    events.blocked();
                    return Ok(());
                }
            }
        }
    }
}

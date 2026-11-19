// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Windows Winsock FFI glue for the USO/URO IO provider: the `WSARecvMsg`
//! extension lookup, the UDP offload socket options, control-message
//! serialization, and sockaddr conversion.
//!
//! # Windows vs. unix terminology
//!
//! | concept                        | Linux                     | Windows                            |
//! |--------------------------------|---------------------------|------------------------------------|
//! | segmentation offload (TX)      | GSO, `UDP_SEGMENT` cmsg   | USO, `UDP_SEND_MSG_SIZE` cmsg      |
//! | coalescing offload (RX)        | GRO, `UDP_GRO` sockopt+cmsg | URO, `UDP_RECV_MAX_COALESCED_SIZE` sockopt / `UDP_COALESCED_INFO` cmsg |
//! | ECN on send                    | `IP_TOS` cmsg (full TOS)  | `IP_ECN` / `IPV6_ECN` cmsg (ECN bits only) |
//! | ECN on recv                    | `IP_RECVTOS`              | `IP_RECVECN` / `IPV6_RECVECN`      |
//! | scatter/gather message         | `msghdr` + `iovec`        | `WSAMSG` + `WSABUF`                |
//! | control message header         | `cmsghdr` (`socklen_t` len) | `CMSGHDR` (`SIZE_T` len)         |
//!
//! The structural differences that matter for this module:
//!
//! * `CMSGHDR::cmsg_len` is pointer-sized (`SIZE_T`) on Windows, where Linux
//!   uses `size_t` on 64-bit but `socklen_t` (32-bit) in some ABIs. The
//!   alignment macro differs too: `WSA_CMSG_ALIGN` rounds to
//!   `MAX_NATURAL_ALIGNMENT` (the pointer size), see `wsacmsg::cmsg_align`.
//! * `WSAMSG::Control` is a `WSABUF` (ptr + `u32` len), not a separate
//!   pointer/length pair, and Winsock writes the consumed length back into
//!   `Control.len` on receive.
//! * Winsock error codes come from `WSAGetLastError()`, not `errno`/
//!   `GetLastError()`, and are in the 10000+ range (`WSAEWOULDBLOCK` = 10035).
//! * Offload availability is a *runtime* property on Windows (it depends on the
//!   OS build and the installed Winsock provider / LSP), so each capability is
//!   probed with `setsockopt` rather than detected at compile time like the
//!   `s2n_quic_platform_gso` cfg on Linux.

use crate::message::wsacmsg::{cmsg_align, read_bytes, read_i32, read_u32, ControlMessages};
use core::ffi::c_void;
use s2n_quic_core::inet::{
    ExplicitCongestionNotification, IpAddress, IpV4Address, IpV6Address, SocketAddress,
    SocketAddressV4, SocketAddressV6,
};
use std::{
    io,
    mem::size_of,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    ptr::{copy_nonoverlapping, null_mut},
};
use windows_sys::{
    core::GUID,
    Win32::{
        Networking::WinSock::{
            setsockopt, WSAGetLastError, WSAIoctl, WSASendMsg, AF_INET, AF_INET6, CMSGHDR,
            IN6_ADDR, IN6_ADDR_0, IN_ADDR, IN_ADDR_0, IPPROTO_IP, IPPROTO_IPV6, IPPROTO_UDP,
            IPV6_DONTFRAG, IPV6_ECN, IPV6_MTU_DISCOVER, IPV6_PKTINFO, IPV6_RECVECN,
            IP_DONTFRAGMENT, IP_ECN, IP_MTU_DISCOVER, IP_PKTINFO, IP_PMTUDISC_PROBE, IP_RECVECN,
            LPFN_WSARECVMSG, LPWSAOVERLAPPED_COMPLETION_ROUTINE,
            SIO_GET_EXTENSION_FUNCTION_POINTER, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKET,
            WSABUF, WSAID_WSARECVMSG, WSAMSG,
        },
        System::IO::OVERLAPPED,
    },
};

// These constants are not exposed by `windows-sys`, so they are transcribed
// from the Windows SDK headers.

// ws2ipdef.h: sets the segment size used by USO sends. Used both as a
// `setsockopt` option (to probe support) and as a per-send control message
// type. This is the Windows counterpart of Linux `UDP_SEGMENT`.
const UDP_SEND_MSG_SIZE: i32 = 2;
// ws2ipdef.h: enables URO receive coalescing up to the given size. The
// counterpart of Linux `UDP_GRO`, except that Linux takes a boolean and lets
// the kernel pick the limit, while Windows takes the maximum coalesced size.
const UDP_RECV_MAX_COALESCED_SIZE: i32 = 3;
// ws2ipdef.h: control message reporting the URO segment size. Note the numeric
// collision with `UDP_RECV_MAX_COALESCED_SIZE`: they share the value 3 but live
// in different namespaces (sockopt vs. cmsg type), so they must not be
// interchanged.
const UDP_COALESCED_INFO: i32 = 3;
// ws2def.h: `WSAMSG` output flag indicating control messages were discarded
// because the buffer was too small. Same semantics as unix `MSG_CTRUNC`, but
// reported in `WSAMSG::dwFlags` rather than `msghdr::msg_flags`.
const MSG_CTRUNC: u32 = 0x200;

/// Large enough for the largest sockaddr Winsock returns here: `SOCKADDR_IN6` is
/// 28 bytes, while `SOCKADDR_IN` is smaller.
pub(crate) const SOCKADDR_STORAGE_LEN: usize = size_of::<SOCKADDR_IN6>();

/// Enough for the largest control payload we emit or receive: the USO/URO
/// segment size and one ECN marking (a 16B header plus 4B of data, aligned to
/// 24B each) plus one `IN6_PKTINFO` (40B), for 88B.
///
/// Under-sizing this makes Winsock silently drop control messages and set
/// `MSG_CTRUNC`, so it is checked against the messages we actually parse.
pub(crate) const CONTROL_BUFFER_CAPACITY: usize = 96;
const _: () = assert!(
    CONTROL_BUFFER_CAPACITY
        >= 2 * cmsg_align(size_of::<CMSGHDR>() + size_of::<u32>())
            // `IN6_PKTINFO` is a 16B address followed by a 4B interface index
            + cmsg_align(size_of::<CMSGHDR>() + size_of::<IN6_ADDR>() + size_of::<u32>())
);

pub(crate) const SOCKET_ERROR: i32 = -1;
pub(crate) const WSAEWOULDBLOCK: i32 = 10035;

/// Signature of the `WSARecvMsg` extension function.
///
/// It is declared here rather than imported because `windows-sys` only exposes
/// the `LPFN_WSARECVMSG` function-pointer type (the symbol itself is not
/// exported from `ws2_32.dll`; see [`load_wsarecvmsg`]). The `system` ABI is
/// `stdcall` on 32-bit Windows, which matters for the callee-cleanup convention.
///
/// The last two arguments support overlapped (IOCP) completion; they are always
/// null/`None` here since the socket is in non-blocking mode and readiness is
/// driven by tokio.
pub(crate) type WsaRecvMsgFn = unsafe extern "system" fn(
    s: SOCKET,
    lpmsg: *mut WSAMSG,
    lpdwnumberofbytesrecvd: *mut u32,
    lpoverlapped: *mut OVERLAPPED,
    lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
) -> i32;

pub(crate) struct RecvMeta {
    pub(crate) len: usize,
    pub(crate) remote: SocketAddress,
}

/// Which ECN socket options the stack accepted.
///
/// Windows only started supporting ECN in Windows 10 2004 and some Winsock
/// providers (e.g. Wine) never implement it, in which case the ECN control
/// message must be omitted from sends: `WSASendMsg` fails the entire datagram
/// otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EcnSupport {
    pub(crate) v4: bool,
    pub(crate) v6: bool,
}

impl EcnSupport {
    /// Returns if ECN can be requested for a datagram sent to `dest`.
    ///
    /// IPv4-mapped addresses are sent over the IPv4 stack, so they are gated on
    /// (and marked with) the IPv4 option.
    #[inline]
    pub(crate) fn is_supported(&self, dest: IpAddress) -> bool {
        match dest.unmap() {
            IpAddress::Ipv4(_) => self.v4,
            IpAddress::Ipv6(_) => self.v6,
        }
    }

    #[inline]
    pub(crate) fn is_any(&self) -> bool {
        self.v4 || self.v6
    }
}

/// A `sockaddr` scratch buffer with the alignment required by the sockaddr
/// types that Winsock writes into it.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub(crate) struct SockAddrBuf {
    bytes: [u8; SOCKADDR_STORAGE_LEN],
}

impl Default for SockAddrBuf {
    #[inline]
    fn default() -> Self {
        Self {
            bytes: [0; SOCKADDR_STORAGE_LEN],
        }
    }
}

#[inline]
fn set_sockopt<T>(sock: SOCKET, level: i32, opt: i32, value: &T) -> bool {
    let rc = unsafe {
        setsockopt(
            sock,
            level,
            opt,
            value as *const T as *const u8,
            size_of::<T>() as i32,
        )
    };
    rc == 0
}

#[inline]
fn last_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

#[inline]
fn would_block() -> io::Error {
    io::Error::from(io::ErrorKind::WouldBlock)
}

/// Probes whether the stack supports UDP segmentation offload (Windows 10
/// 2004+/Server 2022+). Setting the segment size to 0 is a no-op on supported
/// systems and fails on older ones.
///
/// There is no compile-time equivalent of the `s2n_quic_platform_gso` cfg here:
/// the binary may be built on a newer SDK than the machine it runs on, and
/// third-party Winsock providers can reject the option even on new builds, so
/// the probe has to happen against a real socket at startup.
pub(crate) fn supports_uso(sock: SOCKET) -> bool {
    set_sockopt(sock, IPPROTO_UDP, UDP_SEND_MSG_SIZE, &0u32)
}

/// Enables UDP receive coalescing (URO) up to `max` bytes. Returns whether it
/// was accepted (false on Windows without URO support).
///
/// The caller must provide receive buffers of at least `max` bytes: Winsock
/// coalesces up to this size regardless of the buffer passed to `WSARecvMsg`
/// and fails the read with `WSAEMSGSIZE` if it doesn't fit.
///
/// This differs from Linux `UDP_GRO`, where the kernel bounds coalescing by the
/// buffer supplied to each `recvmsg` and simply stops appending segments when it
/// runs out of room. On Windows the limit is a property of the socket, so it has
/// to be agreed with the ring buffer sizing in `io::tokio` up front.
pub(crate) fn enable_uro(sock: SOCKET, max: u32) -> bool {
    set_sockopt(sock, IPPROTO_UDP, UDP_RECV_MAX_COALESCED_SIZE, &max)
}

/// Enables ECN control messages for IPv4 and IPv6 datagrams and reports which
/// levels the stack accepted.
///
/// Dual-stack sockets require the two protocol levels to be set separately and
/// single-stack sockets reject the irrelevant level, so each is tracked
/// independently. The result also gates the ECN control message on send, since
/// providers without ECN support fail `WSASendMsg` outright when it is present.
///
/// Windows exposes the ECN bits on their own (`IP_RECVECN`/`IP_ECN`) instead of
/// the whole TOS/traffic-class byte the way unix does with
/// `IP_RECVTOS`/`IPV6_RECVTCLASS`, so no masking is needed when parsing: the
/// control message payload is already the 2-bit ECN codepoint widened to an
/// `i32`.
pub(crate) fn configure_ecn(sock: SOCKET) -> EcnSupport {
    let enabled = 1i32;
    EcnSupport {
        v4: set_sockopt(sock, IPPROTO_IP, IP_RECVECN, &enabled),
        v6: set_sockopt(sock, IPPROTO_IPV6, IPV6_RECVECN, &enabled),
    }
}

/// Enables the control message reporting the local address and interface a
/// datagram arrived on, and returns whether either level was accepted.
///
/// As with ECN, dual-stack sockets need both protocol levels set and
/// single-stack sockets reject the irrelevant one.
///
/// This is needed for QUIC path validation on wildcard-bound sockets: the
/// endpoint must reply from the same local address the client sent to. Unlike
/// unix, the reply address is *not* set from this: Windows honours the source
/// address selected by the routing table, so the value is only used to populate
/// the path's local address.
pub(crate) fn configure_pktinfo(sock: SOCKET) -> bool {
    let enabled = 1i32;
    let v4 = set_sockopt(sock, IPPROTO_IP, IP_PKTINFO, &enabled);
    let v6 = set_sockopt(sock, IPPROTO_IPV6, IPV6_PKTINFO, &enabled);
    v4 || v6
}

/// Suspends the stack's own path MTU handling so DPLPMTUD can control the
/// packet size, and reports whether either protocol level was configured.
///
/// `IP_MTU_DISCOVER`/`IP_PMTUDISC_PROBE` has the same meaning as on Linux: for
/// datagram sockets it sets the DF bit on every packet and bounds sends by the
/// *interface* MTU rather than the cached *path* MTU, so a probe larger than the
/// stack's PMTU estimate still reaches the wire.
///
/// The option is not implemented on older Windows versions (`setsockopt` fails
/// with `WSAENOPROTOOPT`), so each level falls back to `IP_DONTFRAGMENT` /
/// `IPV6_DONTFRAG`. Those only set the DF bit: the stack keeps enforcing its
/// cached path MTU and rejects larger datagrams locally with `WSAEMSGSIZE`.
/// DPLPMTUD still converges in that case, since a rejected probe is
/// indistinguishable from a lost one, but the send path must not mistake the
/// error for a segmentation offload failure - see the error handling in
/// `io::tokio::task::win::TxSocket::send`.
///
/// As with ECN, dual-stack sockets need both protocol levels set and
/// single-stack sockets reject the irrelevant one.
pub(crate) fn configure_mtu_disc(sock: SOCKET) -> bool {
    let enabled = 1i32;

    let v4 = set_sockopt(sock, IPPROTO_IP, IP_MTU_DISCOVER, &IP_PMTUDISC_PROBE)
        || set_sockopt(sock, IPPROTO_IP, IP_DONTFRAGMENT, &enabled);
    let v6 = set_sockopt(sock, IPPROTO_IPV6, IPV6_MTU_DISCOVER, &IP_PMTUDISC_PROBE)
        || set_sockopt(sock, IPPROTO_IPV6, IPV6_DONTFRAG, &enabled);

    v4 || v6
}

/// Issues a single `WSARecvMsg`, filling `buf` with the (possibly coalesced)
/// payload and `control` with the associated control messages.
///
/// `name`/`control` are caller-owned scratch buffers so the hot path performs no
/// allocation. `WSAMSG` itself is a stack temporary: unlike `msghdr` on unix it
/// is not worth caching, since every field except the buffer pointers has to be
/// rewritten per call anyway.
pub(crate) fn recv_message(
    recvmsg: WsaRecvMsgFn,
    raw: SOCKET,
    buf: &mut [u8],
    name: &mut SockAddrBuf,
    control: &mut ControlBuffer,
) -> io::Result<RecvMeta> {
    let mut wsabuf = WSABUF {
        len: buf.len() as u32,
        buf: buf.as_mut_ptr(),
    };
    let mut msg: WSAMSG = unsafe { std::mem::zeroed() };
    // `namelen` is the *capacity* on input and is overwritten with the actual
    // sockaddr length on output; the family field is what we key off of when
    // decoding, so the returned length is ignored
    msg.name = name.bytes.as_mut_ptr() as *mut SOCKADDR;
    msg.namelen = SOCKADDR_STORAGE_LEN as i32;
    // a single `WSABUF` is used: the ring entry payload is one contiguous
    // region, so there is nothing to scatter across
    msg.lpBuffers = &mut wsabuf;
    msg.dwBufferCount = 1;
    msg.Control = WSABUF {
        len: CONTROL_BUFFER_CAPACITY as u32,
        buf: control.as_mut_ptr(),
    };
    // `dwFlags` is an in/out parameter: 0 on input requests no special
    // behaviour, and Winsock reports `MSG_CTRUNC`/`MSG_TRUNC` back through it
    msg.dwFlags = 0;

    let mut recvd: u32 = 0;
    // SAFETY: `msg` points at buffers owned by the caller that outlive the call,
    // and the null overlapped/completion arguments select the synchronous,
    // non-blocking form of the call.
    let rc = unsafe { recvmsg(raw, &mut msg, &mut recvd, null_mut(), None) };
    if rc == SOCKET_ERROR {
        control.set_len(0);
        // `WSAGetLastError` is thread-local like `errno`, but it must be read
        // instead of `GetLastError`/`io::Error::last_os_error` since Winsock
        // does not reliably mirror socket errors into the Win32 error slot.
        // `WSAEWOULDBLOCK` is mapped explicitly so tokio sees a well-known
        // `ErrorKind::WouldBlock` from inside `try_io`.
        return match unsafe { WSAGetLastError() } {
            WSAEWOULDBLOCK => Err(would_block()),
            _ => Err(last_error()),
        };
    }

    // Winsock writes the number of control bytes it produced back into
    // `Control.len`
    control.set_len(msg.Control.len as usize);

    // A truncated control buffer means messages were dropped, which for a
    // URO-coalesced payload would make it impossible to split back into
    // datagrams. Discard the read instead of misinterpreting the payload.
    if msg.dwFlags & MSG_CTRUNC != 0 {
        debug_assert!(
            false,
            "control message buffers should always have enough capacity"
        );
        control.set_len(0);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control message buffer was truncated",
        ));
    }

    Ok(RecvMeta {
        len: recvd as usize,
        remote: sockaddr_to_inet(name)?,
    })
}

/// Sends a single (possibly USO-segmented) datagram.
///
/// `segment_size` enables USO and must be `None` when the payload is a single
/// datagram. `ecn_support` gates the ECN control message on what the stack
/// actually accepted.
///
/// With USO the whole `payload` is handed to the stack as one buffer and split
/// into `segment_size`-sized datagrams, with a possibly shorter final segment —
/// the same contract as Linux GSO. The total is bounded by `u16::MAX` because it
/// still has to fit a single IP payload before segmentation.
///
/// `WSASendMsg` is exported by `ws2_32.dll` (unlike `WSARecvMsg`), so it can be
/// called directly without an extension lookup.
pub(crate) fn send_message(
    raw: SOCKET,
    payload: &[u8],
    segment_size: Option<usize>,
    ecn: ExplicitCongestionNotification,
    dest: &SocketAddress,
    ecn_support: EcnSupport,
) -> io::Result<()> {
    let mut name = SockAddrBuf::default();
    let namelen = fill_sockaddr(dest, &mut name);

    let mut wsabuf = WSABUF {
        len: payload.len() as u32,
        buf: payload.as_ptr() as *mut u8,
    };
    let mut control = ControlBuffer::encode(segment_size, ecn, dest.ip(), ecn_support)?;

    let mut msg: WSAMSG = unsafe { std::mem::zeroed() };
    msg.name = name.bytes.as_mut_ptr() as *mut SOCKADDR;
    msg.namelen = namelen;
    msg.lpBuffers = &mut wsabuf;
    msg.dwBufferCount = 1;
    // A zero-length control buffer must also have a null pointer: some
    // providers validate the pointer even when the length is 0.
    msg.Control = if control.len() > 0 {
        WSABUF {
            len: control.len() as u32,
            buf: control.as_mut_ptr(),
        }
    } else {
        WSABUF {
            len: 0,
            buf: null_mut(),
        }
    };

    let mut sent: u32 = 0;
    let rc = unsafe { WSASendMsg(raw, &msg, 0, &mut sent, null_mut(), None) };
    if rc == SOCKET_ERROR {
        match unsafe { WSAGetLastError() } {
            WSAEWOULDBLOCK => Err(would_block()),
            _ => Err(last_error()),
        }
    } else {
        Ok(())
    }
}

/// A control message buffer with the alignment required by `WSA_CMSG_ALIGN`.
///
/// Winsock writes `CMSGHDR` records (which lead with a pointer-sized length)
/// directly into this buffer, so it cannot be a plain byte array.
///
/// Serialization is done by hand rather than through the `WSA_CMSG_*` macros:
/// those are C macros with no exported symbols, so there is nothing to link
/// against. This mirrors what `message::cmsg::Encoder` does for unix `cmsghdr`,
/// with the layout differences noted in the module docs.
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub(crate) struct ControlBuffer {
    bytes: [u8; CONTROL_BUFFER_CAPACITY],
    len: usize,
}

impl Default for ControlBuffer {
    #[inline]
    fn default() -> Self {
        Self {
            bytes: [0u8; CONTROL_BUFFER_CAPACITY],
            len: 0,
        }
    }
}

impl ControlBuffer {
    /// Encodes the control messages for a single send.
    fn encode(
        segment_size: Option<usize>,
        ecn: ExplicitCongestionNotification,
        dest_ip: IpAddress,
        ecn_support: EcnSupport,
    ) -> io::Result<Self> {
        let mut out = Self::default();
        let mut off = 0;

        if let Some(segment_size) = segment_size {
            let segment_size = (segment_size as u32).to_ne_bytes();
            off = out.write(off, IPPROTO_UDP, UDP_SEND_MSG_SIZE, &segment_size)?;
        }

        // Windows rejects app-specified CE markings with WSAEINVAL; only ECT
        // markings are valid on send. Marking CE is the router's job, and the
        // stack enforces that. Linux accepts the full TOS byte via `IP_TOS` and
        // performs no such validation, so this check is Windows-specific.
        // https://learn.microsoft.com/en-us/windows/win32/winsock/winsock-ecn
        let is_ect = matches!(
            ecn,
            ExplicitCongestionNotification::Ect0 | ExplicitCongestionNotification::Ect1
        );

        if is_ect && ecn_support.is_supported(dest_ip) {
            let value = (ecn as i32).to_ne_bytes();
            // IPv4-mapped destinations travel over the IPv4 stack and must be
            // marked at the IPv4 level
            let (level, ty) = match dest_ip.unmap() {
                IpAddress::Ipv4(_) => (IPPROTO_IP, IP_ECN),
                IpAddress::Ipv6(_) => (IPPROTO_IPV6, IPV6_ECN),
            };
            off = out.write(off, level, ty, &value)?;
        }

        out.len = off;
        Ok(out)
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Records how many bytes Winsock wrote into the buffer.
    #[inline]
    pub(crate) fn set_len(&mut self, len: usize) {
        self.len = len.min(CONTROL_BUFFER_CAPACITY);
    }

    /// The control messages currently held by the buffer.
    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    #[inline]
    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }

    /// Appends one `CMSGHDR` + payload record at `off` and returns the offset of
    /// the next record.
    ///
    /// `cmsg_len` covers the header plus the unpadded data (matching
    /// `WSA_CMSG_LEN`), while the *next* record starts at the `WSA_CMSG_ALIGN`
    /// rounded-up offset. Getting this wrong makes the provider either skip the
    /// message or reject the send with `WSAEINVAL`.
    fn write(&mut self, off: usize, level: i32, ty: i32, data: &[u8]) -> io::Result<usize> {
        let hdr_len = size_of::<CMSGHDR>();
        let len = hdr_len.checked_add(data.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "control message length overflow",
            )
        })?;
        let end = off.checked_add(cmsg_align(len)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "control message alignment overflow",
            )
        })?;
        if end > self.bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control buffer capacity exceeded",
            ));
        }

        let hdr = CMSGHDR {
            cmsg_len: len,
            cmsg_level: level,
            cmsg_type: ty,
        };
        // SAFETY: the bounds check above guarantees the header and data fit.
        // `write_unaligned` is used defensively; the buffer is 8-byte aligned
        // and `off` is always `cmsg_align`ed, so this compiles to an aligned
        // store in practice.
        unsafe {
            (self.bytes.as_mut_ptr().add(off) as *mut CMSGHDR).write_unaligned(hdr);
            copy_nonoverlapping(
                data.as_ptr(),
                self.bytes.as_mut_ptr().add(off + cmsg_align(hdr_len)),
                data.len(),
            );
        }
        Ok(end)
    }
}

/// Parses the `UDP_COALESCED_INFO` control message, returning the coalesced
/// segment size if present.
///
/// Absence of the message means the read was not coalesced (either URO is off,
/// or the stack had only one datagram to give), not that the size is zero.
pub(crate) fn parse_coalesced(control: &[u8]) -> Option<u32> {
    ControlMessages::new(control)
        .find(|msg| msg.level == IPPROTO_UDP && msg.ty == UDP_COALESCED_INFO)
        .and_then(|msg| read_u32(msg.data))
}

/// Parses the local address out of an `IP_PKTINFO` / `IPV6_PKTINFO` control
/// message, i.e. the address the datagram was actually sent to.
///
/// The returned address has no port, since the control message doesn't carry
/// one; the caller fills it in from the socket's bound address.
pub(crate) fn parse_pktinfo(control: &[u8]) -> Option<SocketAddress> {
    // the pktinfo payload only carries the destination IP (and interface
    // index), so there is no port to read here. `message::wsamsg::Message::rx_read`
    // replaces this placeholder with the socket's bound port.
    const PORT: u16 = 0;

    // both levels use the same option value, so they can only be told apart by
    // the level itself
    for msg in ControlMessages::new(control) {
        if msg.level == IPPROTO_IP && msg.ty == IP_PKTINFO {
            if let Some(addr) = read_bytes::<4>(msg.data) {
                return Some(SocketAddressV4::new(IpV4Address::new(addr), PORT).into());
            }
        }

        if msg.level == IPPROTO_IPV6 && msg.ty == IPV6_PKTINFO {
            if let Some(addr) = read_bytes::<16>(msg.data) {
                return Some(SocketAddressV6::new(IpV6Address::new(addr), PORT).into());
            }
        }
    }

    None
}

/// Parses an `IP_ECN` / `IPV6_ECN` control message from `WSARecvMsg`.
///
/// The payload is already just the ECN codepoint (0-3) as an `i32`, so unlike
/// the unix `IP_TOS`/`IPV6_TCLASS` path there is no need to mask off the DSCP
/// bits. A missing message is reported as `0` (`NotEct`).
///
/// With URO the marking applies to the entire coalesced run: the stack only
/// coalesces datagrams that share the same ECN codepoint.
pub(crate) fn parse_ecn(control: &[u8]) -> u8 {
    ControlMessages::new(control)
        .find(|msg| {
            (msg.level == IPPROTO_IP && msg.ty == IP_ECN)
                || (msg.level == IPPROTO_IPV6 && msg.ty == IPV6_ECN)
        })
        .and_then(|msg| read_i32(msg.data))
        .map(|ecn| ecn as u8)
        .unwrap_or(0)
}

/// Fills a sockaddr in `buf` from the s2n-quic `SocketAddress`. Returns its length.
fn fill_sockaddr(addr: &SocketAddress, buf: &mut SockAddrBuf) -> i32 {
    let port = addr.port();
    match addr.ip() {
        IpAddress::Ipv4(v4) => unsafe {
            let mut sa: SOCKADDR_IN = std::mem::zeroed();
            sa.sin_family = AF_INET;
            sa.sin_port = port.to_be();
            let mut octets = [0u8; 4];
            octets.copy_from_slice(v4.as_bytes());
            sa.sin_addr = IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: u32::from_ne_bytes(octets),
                },
            };
            copy_nonoverlapping(
                &sa as *const SOCKADDR_IN as *const u8,
                buf.bytes.as_mut_ptr(),
                size_of::<SOCKADDR_IN>(),
            );
            size_of::<SOCKADDR_IN>() as i32
        },
        IpAddress::Ipv6(v6) => unsafe {
            let mut sa: SOCKADDR_IN6 = std::mem::zeroed();
            sa.sin6_family = AF_INET6;
            sa.sin6_port = port.to_be();
            let mut octets = [0u8; 16];
            octets.copy_from_slice(v6.as_bytes());
            sa.sin6_addr = IN6_ADDR {
                u: IN6_ADDR_0 { Byte: octets },
            };
            copy_nonoverlapping(
                &sa as *const SOCKADDR_IN6 as *const u8,
                buf.bytes.as_mut_ptr(),
                size_of::<SOCKADDR_IN6>(),
            );
            size_of::<SOCKADDR_IN6>() as i32
        },
    }
}

/// Decodes a raw sockaddr (v4/v6) into an s2n-quic `SocketAddress`.
fn sockaddr_to_inet(name: &SockAddrBuf) -> io::Result<SocketAddress> {
    let name = &name.bytes;
    let family = u16::from_ne_bytes([name[0], name[1]]);
    let std_addr = match family {
        AF_INET6 => {
            let sa = unsafe { (name.as_ptr() as *const SOCKADDR_IN6).read_unaligned() };
            let octets: [u8; 16] = unsafe { sa.sin6_addr.u.Byte };
            SocketAddr::new(
                IpAddr::V6(Ipv6Addr::from(octets)),
                u16::from_be(sa.sin6_port),
            )
        }
        AF_INET => {
            let sa = unsafe { (name.as_ptr() as *const SOCKADDR_IN).read_unaligned() };
            let octets = unsafe { sa.sin_addr.S_un.S_addr }.to_ne_bytes();
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::from(octets)),
                u16::from_be(sa.sin_port),
            )
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported sockaddr family",
            ));
        }
    };
    Ok(std_addr.into())
}

/// Looks up the `WSARecvMsg` extension function pointer for `sock`.
///
/// Winsock "extension functions" (`WSARecvMsg`, `AcceptEx`, `ConnectEx`, ...)
/// are not exported from `ws2_32.dll`; they belong to the underlying service
/// provider and are obtained per socket by passing the well-known GUID
/// (`WSAID_WSARECVMSG`) to `WSAIoctl` with
/// `SIO_GET_EXTENSION_FUNCTION_POINTER`. This has no unix analogue, where
/// `recvmsg` is simply a libc symbol.
///
/// A `None` result means the provider doesn't implement it (e.g. some LSPs or
/// emulation layers), in which case the receive path degrades to `recv_from`
/// with no ECN, pktinfo or URO metadata.
pub(crate) fn load_wsarecvmsg(sock: SOCKET) -> Option<WsaRecvMsgFn> {
    let mut func: LPFN_WSARECVMSG = None;
    let mut bytes: u32 = 0;
    let rc = unsafe {
        WSAIoctl(
            sock,
            SIO_GET_EXTENSION_FUNCTION_POINTER,
            &WSAID_WSARECVMSG as *const GUID as *const c_void,
            size_of::<GUID>() as u32,
            &mut func as *mut LPFN_WSARECVMSG as *mut c_void,
            size_of::<LPFN_WSARECVMSG>() as u32,
            &mut bytes,
            null_mut(),
            None,
        )
    };
    if rc == 0 {
        func
    } else {
        None
    }
}

#[cfg(test)]
#[allow(non_snake_case)] // tests are named `scenario__action__expectation`
mod tests {
    use super::*;

    const ECN_SUPPORTED: EcnSupport = EcnSupport { v4: true, v6: true };

    fn addr(value: &str) -> SocketAddress {
        value.parse::<SocketAddr>().unwrap().into()
    }

    #[test]
    fn dual_stack_socket__configure_mtu_disc__suspends_stack_pmtu() {
        let socket = crate::syscall::bind_udp("[::]:0", false, false, false).unwrap();

        // either `IP_PMTUDISC_PROBE` or the `IP_DONTFRAGMENT` fallback has to
        // stick, otherwise `io::tokio` drops the endpoint to the minimum MTU
        assert!(crate::syscall::configure_mtu_disc(&socket));
    }

    #[test]
    fn uso_and_ecn__encode_control__round_trips() {
        let control = ControlBuffer::encode(
            Some(1200),
            ExplicitCongestionNotification::Ect0,
            addr("127.0.0.1:443").ip(),
            ECN_SUPPORTED,
        )
        .unwrap();

        let messages: Vec<_> = ControlMessages::new(control.as_slice()).collect();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].level, IPPROTO_UDP);
        assert_eq!(messages[0].ty, UDP_SEND_MSG_SIZE);
        assert_eq!(read_u32(messages[0].data), Some(1200));
        assert_eq!(messages[1].level, IPPROTO_IP);
        assert_eq!(messages[1].ty, IP_ECN);
        assert_eq!(
            read_i32(messages[1].data),
            Some(ExplicitCongestionNotification::Ect0 as i32)
        );
    }

    #[test]
    fn ipv6_destination__encode_control__uses_v6_level() {
        let control = ControlBuffer::encode(
            None,
            ExplicitCongestionNotification::Ect1,
            addr("[2001:db8::1]:443").ip(),
            ECN_SUPPORTED,
        )
        .unwrap();

        let messages: Vec<_> = ControlMessages::new(control.as_slice()).collect();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].level, IPPROTO_IPV6);
        assert_eq!(messages[0].ty, IPV6_ECN);
    }

    #[test]
    fn ipv4_mapped_destination__encode_control__uses_v4_level() {
        let control = ControlBuffer::encode(
            None,
            ExplicitCongestionNotification::Ect0,
            addr("[::ffff:127.0.0.1]:443").ip(),
            ECN_SUPPORTED,
        )
        .unwrap();

        let messages: Vec<_> = ControlMessages::new(control.as_slice()).collect();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].level, IPPROTO_IP);
        assert_eq!(messages[0].ty, IP_ECN);
    }

    #[test]
    fn congestion_experienced__encode_control__omits_ecn() {
        let control = ControlBuffer::encode(
            None,
            ExplicitCongestionNotification::Ce,
            addr("127.0.0.1:443").ip(),
            ECN_SUPPORTED,
        )
        .unwrap();

        assert_eq!(control.len(), 0);
    }

    #[test]
    fn unsupported_stack__encode_control__omits_ecn() {
        let control = ControlBuffer::encode(
            None,
            ExplicitCongestionNotification::Ect0,
            addr("127.0.0.1:443").ip(),
            EcnSupport {
                v4: false,
                v6: true,
            },
        )
        .unwrap();

        assert_eq!(control.len(), 0);
    }

    #[test]
    fn ipv4_and_ipv6__fill_and_parse_sockaddr__round_trips() {
        for value in ["127.0.0.1:4433", "[2001:db8::1]:4433"] {
            let expected = addr(value);
            let mut buf = SockAddrBuf::default();
            let len = fill_sockaddr(&expected, &mut buf);

            assert!(len as usize <= SOCKADDR_STORAGE_LEN);
            assert_eq!(sockaddr_to_inet(&buf).unwrap(), expected);
        }
    }

    #[test]
    fn unknown_family__parse_sockaddr__errors() {
        let buf = SockAddrBuf::default();
        assert!(sockaddr_to_inet(&buf).is_err());
    }

    /// Builds an `IP_PKTINFO`/`IPV6_PKTINFO` control message the way Winsock
    /// would report one: the local address followed by an interface index.
    fn pktinfo(level: i32, ty: i32, addr: &[u8]) -> Vec<u8> {
        let mut data = addr.to_vec();
        data.extend_from_slice(&7u32.to_ne_bytes());

        let mut out = ControlBuffer::default();
        out.len = out.write(0, level, ty, &data).unwrap();
        out.as_slice().to_vec()
    }

    #[test]
    fn ipv4_pktinfo__parse__returns_local_address() {
        let control = pktinfo(IPPROTO_IP, IP_PKTINFO, &[192, 168, 0, 1]);

        assert_eq!(
            parse_pktinfo(&control).map(|addr| addr.ip()),
            Some(addr("192.168.0.1:0").ip())
        );
    }

    #[test]
    fn ipv6_pktinfo__parse__returns_local_address() {
        let expected = addr("[2001:db8::1]:0");
        let octets = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).octets();
        let control = pktinfo(IPPROTO_IPV6, IPV6_PKTINFO, &octets);

        assert_eq!(
            parse_pktinfo(&control).map(|addr| addr.ip()),
            Some(expected.ip())
        );
    }

    #[test]
    fn truncated_pktinfo__parse__returns_none() {
        let mut control = pktinfo(IPPROTO_IP, IP_PKTINFO, &[192, 168, 0, 1]);
        control.truncate(size_of::<CMSGHDR>() + 2);

        assert_eq!(parse_pktinfo(&control), None);
    }

    #[test]
    fn no_pktinfo__parse__returns_none() {
        let control = ControlBuffer::encode(
            Some(1200),
            ExplicitCongestionNotification::Ect0,
            addr("127.0.0.1:443").ip(),
            ECN_SUPPORTED,
        )
        .unwrap();

        assert_eq!(parse_pktinfo(control.as_slice()), None);
    }

    #[test]
    fn coalesced_and_ecn_messages__parse__returns_values() {
        let control = ControlBuffer::encode(
            Some(1200),
            ExplicitCongestionNotification::Ect0,
            addr("[2001:db8::1]:443").ip(),
            ECN_SUPPORTED,
        )
        .unwrap();

        assert_eq!(
            parse_ecn(control.as_slice()),
            ExplicitCongestionNotification::Ect0 as u8
        );
        // the TX path encodes `UDP_SEND_MSG_SIZE`, which is not the RX coalescing
        // message, so it should not be picked up
        assert_eq!(parse_coalesced(control.as_slice()), None);
    }
}

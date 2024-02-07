// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// some platforms contain empty implementations so disable any warnings from those
#![allow(unused_variables, unused_macros, unused_mut, clippy::let_and_return)]

use core::ops::ControlFlow;
use socket2::{Domain, Protocol, Socket, Type};
use std::io;

/// Calls the given libc function and wraps the result in an `io::Result`.
macro_rules! libc {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        let res = unsafe { libc::$fn($($arg, )*) };
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}

#[cfg(s2n_quic_platform_socket_mmsg)]
pub mod mmsg;
#[cfg(s2n_quic_platform_socket_msg)]
pub mod msg;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // this isn't constructed on some platforms
pub enum SocketType {
    Blocking,
    NonBlocking,
}

pub trait SocketEvents {
    /// Called when `count` packets are completed
    ///
    /// If `Continue` is returned, the socket will assume the packet was acceptable and continue
    /// with the remaining packets. If `Break` is returned, the syscall stop looping and yield to
    /// the caller.
    fn on_complete(&mut self, count: usize) -> ControlFlow<(), ()>;

    /// Called when an error occurs on a socket
    ///
    /// If `Continue` is returned, the socket will discard the packet and continue
    /// with the remaining packets. If `Break` is returned, the syscall will assume the current
    /// packet can be retried and yield to the caller.
    fn on_error(&mut self, error: io::Error) -> ControlFlow<(), ()>;
}

#[cfg(unix)]
pub trait UnixMessage: crate::message::Message {
    fn send<E: SocketEvents>(fd: std::os::unix::io::RawFd, entries: &mut [Self], events: &mut E);
    fn recv<E: SocketEvents>(
        fd: std::os::unix::io::RawFd,
        ty: SocketType,
        entries: &mut [Self],
        events: &mut E,
    );
}

pub fn udp_socket(addr: std::net::SocketAddr) -> io::Result<Socket> {
    let domain = Domain::for_address(addr);
    let socket_type = Type::DGRAM;
    let protocol = Some(Protocol::UDP);

    let socket = Socket::new(domain, socket_type, protocol)?;

    // allow ipv4 to also connect - ignore the error if it fails
    let _ = socket.set_only_v6(false);

    Ok(socket)
}

/// Creates a UDP socket bound to the provided address
pub fn bind_udp<A: std::net::ToSocketAddrs>(
    addr: A,
    reuse_address: bool,
    reuse_port: bool,
) -> io::Result<Socket> {
    let addr = addr.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            io::ErrorKind::InvalidInput,
            "the provided bind address was empty",
        )
    })?;
    let socket = udp_socket(addr)?;

    socket.set_reuse_address(reuse_address)?;

    #[cfg(unix)]
    socket.set_reuse_port(reuse_port)?;

    // mark the variable as "used" regardless of platform support
    let _ = reuse_port;

    socket.bind(&addr.into())?;

    Ok(socket)
}

/// Binds a socket to a specified interface by name
#[cfg(feature = "xdp")]
pub fn bind_to_interface<F: std::os::unix::io::AsRawFd>(
    socket: &F,
    ifname: &std::ffi::CStr,
) -> io::Result<()> {
    libc!(setsockopt(
        socket.as_raw_fd(),
        libc::SOL_SOCKET,
        libc::SO_BINDTODEVICE,
        ifname as *const _ as *const _,
        libc::IF_NAMESIZE as _
    ))?;
    Ok(())
}

/// Disables MTU discovery and fragmentation on the socket
pub fn configure_mtu_disc(tx_socket: &Socket) -> bool {
    let mut success = false;

    //= https://www.rfc-editor.org/rfc/rfc9000#section-14
    //# UDP datagrams MUST NOT be fragmented at the IP layer.

    //= https://www.rfc-editor.org/rfc/rfc9000#section-14
    //# In IPv4 [IPv4], the Don't Fragment (DF) bit MUST be set if possible, to
    //# prevent fragmentation on the path.

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //# In IPv4, a probe packet MUST be sent with the Don't
    //# Fragment (DF) bit set in the IP header and without network layer
    //# endpoint fragmentation.

    //= https://www.rfc-editor.org/rfc/rfc8899#section-4.5
    //# A PL implementing this specification MUST suspend network layer
    //# processing of outgoing packets that enforces a PMTU
    //# [RFC1191][RFC8201] for each flow utilizing DPLPMTUD and instead use
    //# DPLPMTUD to control the size of packets that are sent by a flow.
    #[cfg(s2n_quic_platform_mtu_disc)]
    {
        use std::os::unix::io::AsRawFd;

        // IP_PMTUDISC_PROBE setting will set the DF (Don't Fragment) flag
        // while also ignoring the Path MTU. This means packets will not
        // be fragmented, and the EMSGSIZE error will not be returned for
        // packets larger than the Path MTU according to the kernel.
        success |= libc!(setsockopt(
            tx_socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            &libc::IP_PMTUDISC_PROBE as *const _ as _,
            core::mem::size_of_val(&libc::IP_PMTUDISC_PROBE) as _,
        ))
        .is_ok();

        success |= libc!(setsockopt(
            tx_socket.as_raw_fd(),
            libc::IPPROTO_IPV6,
            libc::IPV6_MTU_DISCOVER,
            &libc::IP_PMTUDISC_PROBE as *const _ as _,
            core::mem::size_of_val(&libc::IP_PMTUDISC_PROBE) as _,
        ))
        .is_ok();
    }

    success
}

/// Configures the socket to return TOS/ECN information as part of the ancillary data
pub fn configure_tos(rx_socket: &Socket) -> bool {
    let mut success = false;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let enabled: libc::c_int = 1;

        if let Some((level, ty)) = crate::features::tos_v4::SOCKOPT {
            success |= libc!(setsockopt(
                rx_socket.as_raw_fd(),
                level,
                ty,
                &enabled as *const _ as _,
                core::mem::size_of_val(&enabled) as _,
            ))
            .is_ok();
        }

        if let Some((level, ty)) = crate::features::tos_v6::SOCKOPT {
            success |= libc!(setsockopt(
                rx_socket.as_raw_fd(),
                level,
                ty,
                &enabled as *const _ as _,
                core::mem::size_of_val(&enabled) as _,
            ))
            .is_ok();
        }
    }

    success
}

/// Configures the socket to return local address and interface information as part of the
/// ancillary data
pub fn configure_pktinfo(rx_socket: &Socket) -> bool {
    let mut success = false;

    // Set up the RX socket to pass information about the local address and interface
    #[cfg(s2n_quic_platform_pktinfo)]
    {
        use std::os::unix::io::AsRawFd;
        let enabled: libc::c_int = 1;

        success |= libc!(setsockopt(
            rx_socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_PKTINFO,
            &enabled as *const _ as _,
            core::mem::size_of_val(&enabled) as _,
        ))
        .is_ok();

        success |= libc!(setsockopt(
            rx_socket.as_raw_fd(),
            libc::IPPROTO_IPV6,
            libc::IPV6_RECVPKTINFO,
            &enabled as *const _ as _,
            core::mem::size_of_val(&enabled) as _,
        ))
        .is_ok();
    }

    success
}

pub fn configure_gro(rx_socket: &Socket) -> bool {
    let mut success = false;

    #[cfg(unix)]
    if let Some((level, ty)) = crate::features::gro::SOCKOPT {
        use std::os::unix::io::AsRawFd;
        let enabled: libc::c_int = 1;

        success |= libc!(setsockopt(
            rx_socket.as_raw_fd(),
            level as _,
            ty as _,
            &enabled as *const _ as _,
            core::mem::size_of_val(&enabled) as _
        ))
        .is_ok();
    }

    success
}

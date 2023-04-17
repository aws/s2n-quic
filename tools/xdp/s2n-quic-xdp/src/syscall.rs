// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{Address, MmapOffsets, RingOffsetV1, SocketOptions, Statistics, UmemReg},
    Result,
};
use core::{mem::size_of, ptr::NonNull};
use libc::{c_void, AF_XDP, SOCK_RAW, SOL_XDP};
use std::{
    io,
    os::unix::io::{AsRawFd, RawFd},
};

/// Calls the given libc function and wraps the result in an `io::Result`.
macro_rules! libc {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        let res = libc::$fn($($arg, )*);
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}

/// Opens an AF_XDP socket
///
/// This call requires `CAP_NET_RAW` capabilities to succeed.
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L327).
pub fn open() -> io::Result<RawFd> {
    unsafe { libc!(socket(AF_XDP, SOCK_RAW, 0)) }
}

/// Returns all of the [`MmapOffsets`] configured for the AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L228).
#[inline]
pub fn offsets<Fd: AsRawFd>(fd: &Fd) -> Result<MmapOffsets> {
    let mut offsets = MmapOffsets::default();

    let optlen = xdp_option(fd, SocketOptions::MmapOffsets, &mut offsets)?;

    // if the written size matched the V2 size, then return
    if optlen == size_of::<MmapOffsets>() {
        return Ok(offsets);
    }

    // adapt older versions of the kernel from v1 to v2
    if optlen == size_of::<MmapOffsets<RingOffsetV1>>() {
        return Ok(offsets.as_v1());
    }

    // an invalid size was returned
    Err(io::Error::new(
        io::ErrorKind::Other,
        format!("invalid mmap offset size: {optlen}"),
    ))
}

/// Returns the collected statistics for the provided AF_XDP socket
#[inline]
pub fn statistics<Fd: AsRawFd>(fd: &Fd) -> Result<Statistics> {
    let mut stats = Statistics::default();
    xdp_option(fd, SocketOptions::Statistics, &mut stats)?;
    Ok(stats)
}

/// Returns the netns cookie associated with the provided AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L1055).
pub fn netns_cookie<Fd: AsRawFd>(fd: &Fd) -> Result<u64> {
    // Rust's `libc` doesn't have this defined so we need to define it here
    // https://github.com/torvalds/linux/blob/e62252bc55b6d4eddc6c2bdbf95a448180d6a08d/include/uapi/asm-generic/socket.h#L125
    const SO_NETNS_COOKIE: libc::c_int = 71;

    let mut cookie = 0u64;
    let mut optlen = size_of::<u64>() as libc::socklen_t;

    let ret = unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            SO_NETNS_COOKIE,
            &mut cookie as *mut _ as _,
            &mut optlen,
        )
    };

    if ret == 0 {
        return Ok(cookie);
    }

    // the cookie syscall is not supported here so return the default value of 1
    // https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L55
    if errno::errno().0 == libc::ENOPROTOOPT {
        return Ok(1);
    }

    Err(io::Error::last_os_error())
}

/// Configures the fill ring size of the provided AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L257).
#[inline]
pub fn set_fill_ring_size<Fd: AsRawFd>(fd: &Fd, len: u32) -> Result<()> {
    set_xdp_option(fd, SocketOptions::UmemFillRing, &len)
}

/// Configures the completion ring size of the provided AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L263).
#[inline]
pub fn set_completion_ring_size<Fd: AsRawFd>(fd: &Fd, len: u32) -> Result<()> {
    set_xdp_option(fd, SocketOptions::UmemCompletionRing, &len)
}

/// Configures the RX ring size of the provided AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L1081).
#[inline]
pub fn set_rx_ring_size<Fd: AsRawFd>(fd: &Fd, len: u32) -> Result<()> {
    set_xdp_option(fd, SocketOptions::RxRing, &len)
}

/// Configures the TX ring size of the provided AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L1093).
#[inline]
pub fn set_tx_ring_size<Fd: AsRawFd>(fd: &Fd, len: u32) -> Result<()> {
    set_xdp_option(fd, SocketOptions::TxRing, &len)
}

/// Configures the UMEM options for the provided AF_XDP socket
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L344).
#[inline]
pub fn set_umem<Fd: AsRawFd>(fd: &Fd, reg: &UmemReg) -> Result<()> {
    set_xdp_option(fd, SocketOptions::UmemReg, reg)
}

/// Binds the provided AF_XDP socket to an address
#[inline]
pub fn bind<Fd: AsRawFd>(fd: &Fd, addr: &mut Address) -> Result<()> {
    unsafe {
        libc!(bind(
            fd.as_raw_fd(),
            addr as *mut _ as _,
            size_of::<Address>() as _
        ))?;
    }
    Ok(())
}

/// Notifies the kernel to send any packets on the TX ring
///
/// This should be called after checking if the TX ring needs a wake up.
#[inline]
pub fn wake_tx<Fd: AsRawFd>(fd: &Fd) -> Result<()> {
    unsafe {
        libc!(sendto(
            fd.as_raw_fd(),
            core::ptr::null_mut(),
            0,
            libc::MSG_DONTWAIT,
            core::ptr::null_mut(),
            0,
        ))?;
    }
    Ok(())
}

/// Tries to receive packets on the RX ring
///
/// Note that this call is non-blocking and is usually used with busy polling. Use [`libc::poll`]
/// instead to block a task on socket readiness.
#[inline]
pub fn busy_poll<Fd: AsRawFd>(fd: &Fd) -> Result<u32> {
    let mut msg = unsafe {
        // Safety: msghdr is zeroable
        core::mem::zeroed()
    };
    let count = unsafe { libc!(recvmsg(fd.as_raw_fd(), &mut msg, libc::MSG_DONTWAIT,))? };
    Ok(count as u32)
}

/// Opens a mmap region, with an optional associated file descriptor.
///
/// See [xsk.c](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L273).
#[inline]
pub fn mmap(len: usize, offset: usize, fd: Option<RawFd>) -> Result<NonNull<c_void>> {
    let flags = if fd.is_some() {
        libc::MAP_SHARED | libc::MAP_POPULATE
    } else {
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS
    };

    // See:
    // * Fill https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L273
    // * Completion https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L287
    // * RX https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L1111
    // * TX https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L1132
    let addr = unsafe {
        libc::mmap(
            core::ptr::null_mut(),
            len as _,
            libc::PROT_READ | libc::PROT_WRITE,
            flags,
            fd.unwrap_or(-1),
            offset as _,
        )
    };

    if addr == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }

    let addr = NonNull::new(addr)
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "mmap returned null pointer"))?;

    Ok(addr)
}

/// Unmaps a mmap region
///
/// # Safety
///
/// The following must be true:
///
/// * The provided `addr` MUST be a mmap'd region
/// * The original mmap length MUST match the provided `len` parameter.
pub unsafe fn munmap(addr: NonNull<c_void>, len: usize) -> Result<()> {
    libc!(munmap(addr.as_ptr(), len as _))?;
    Ok(())
}

#[inline]
fn xdp_option<Fd: AsRawFd, T: Sized>(fd: &Fd, opt: SocketOptions, value: &mut T) -> Result<usize> {
    let mut optlen = size_of::<T>() as libc::socklen_t;

    unsafe {
        libc!(getsockopt(
            fd.as_raw_fd(),
            SOL_XDP,
            opt as _,
            value as *mut _ as _,
            &mut optlen,
        ))?;
    }

    Ok(optlen as usize)
}

#[inline]
fn set_xdp_option<Fd: AsRawFd, T: Sized>(fd: &Fd, opt: SocketOptions, value: &T) -> Result<()> {
    let optlen = size_of::<T>() as libc::socklen_t;

    unsafe {
        libc!(setsockopt(
            fd.as_raw_fd(),
            SOL_XDP,
            opt as _,
            value as *const _ as _,
            optlen,
        ))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mmap::Mmap;
    use core::ffi::CStr;

    #[test]
    fn syscall_test() {
        // This call requires `CAP_NET_RAW`. If the test doesn't have this set, then log and skip
        // the next calls
        let fd = if let Ok(fd) = open() {
            fd
        } else {
            assert!(
                std::env::var("CAP_NET_RAW_ENABLED").is_err(),
                "expected the syscall test to be executed"
            );

            use std::io::Write;
            let _ = writeln!(
                std::io::stdout(),
                "WARNING: CAP_NET_RAW capabilities missing; skipping `syscall_test`",
            );
            return;
        };

        // the ring sizes need to be a power of 2
        let ring_size = 32u32;
        let frame_size = 4096;
        let frame_count = (ring_size * 4) as usize;

        // Set up the ring sizes
        {
            set_fill_ring_size(&fd, ring_size).unwrap();
            set_completion_ring_size(&fd, ring_size).unwrap();
            set_rx_ring_size(&fd, ring_size).unwrap();
            set_tx_ring_size(&fd, ring_size).unwrap();
        }

        // Set up the UMEM region
        let umem_len = frame_size * frame_count;
        let umem = Mmap::new(umem_len, 0, None).unwrap();
        {
            let umem_conf = UmemReg {
                addr: umem.addr().as_ptr() as *mut _ as _,
                chunk_size: frame_size as _,
                flags: Default::default(),
                headroom: 0,
                len: umem_len as _,
            };

            set_umem(&fd, &umem_conf).unwrap();
        }

        // Print out the getter calls
        {
            dbg!(offsets(&fd).unwrap());
            dbg!(statistics(&fd).unwrap());
            dbg!(netns_cookie(&fd).unwrap());
        }

        // try binding to the loopback interface and calling the IO syscalls
        {
            let mut addr = Address::default();
            let if_name = unsafe { CStr::from_ptr(b"lo\0" as *const _ as _) };

            if addr.set_if_name(if_name).is_ok() {
                eprintln!("using ifindex {}", addr.ifindex);
                if bind(&fd, &mut addr).is_ok() {
                    wake_tx(&fd).unwrap();
                    dbg!(busy_poll(&fd).unwrap());
                }
            }
        }

        unsafe {
            let _ = libc!(close(fd));
        }
    }
}

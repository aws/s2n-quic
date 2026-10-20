// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Decoding of Winsock control messages (`WSAMSG::Control`).
//!
//! This is the Windows counterpart of the unix-only `message::cmsg` module, and
//! exists separately because the `WSA_CMSG_*` accessors are C macros in
//! `ws2def.h` with no exported symbols to link against, and because the record
//! layout differs from unix `cmsghdr`:
//!
//! * `cmsg_len` is `SIZE_T` (pointer-sized) rather than `socklen_t`, so the
//!   header is 16 bytes on 64-bit instead of the 12/16 unix mix.
//! * `WSA_CMSG_ALIGN` rounds to `MAX_NATURAL_ALIGNMENT` (the pointer size),
//!   where unix `CMSG_ALIGN` rounds to `sizeof(size_t)`; these happen to agree
//!   on the targets we support, but the constant is spelled out below rather
//!   than assumed.
//!
//! Everything here is decode-only; encoding lives in
//! [`syscall::win::ControlBuffer`](crate::syscall::win), since the set of
//! messages we emit is fixed and small.
//!
//! All parsing is defensive: the buffer comes from the OS but is also exercised
//! by tests with hostile lengths, so a malformed record ends iteration rather
//! than panicking or reading out of bounds.

use std::mem::{align_of, size_of};

/// The byte layout of Winsock's `CMSGHDR`.
///
/// Declared locally instead of using `windows_sys::CMSGHDR` so this module can
/// be unit tested on non-Windows hosts and so the field order/size assumptions
/// are visible at the point of use.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RawCmsgHdr {
    len: usize,
    level: i32,
    ty: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ControlMessage<'a> {
    pub(crate) level: i32,
    pub(crate) ty: i32,
    pub(crate) data: &'a [u8],
}

/// Iterates the `CMSGHDR` records in a control buffer, i.e. the equivalent of
/// the `WSA_CMSG_FIRSTHDR`/`WSA_CMSG_NXTHDR` loop.
pub(crate) struct ControlMessages<'a> {
    control: &'a [u8],
    off: usize,
}

impl<'a> ControlMessages<'a> {
    pub(crate) fn new(control: &'a [u8]) -> Self {
        Self { control, off: 0 }
    }
}

impl<'a> Iterator for ControlMessages<'a> {
    type Item = ControlMessage<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let header_len = size_of::<RawCmsgHdr>();
        let remaining = self.control.get(self.off..)?;
        if remaining.len() < header_len {
            return None;
        }

        let hdr = read_at::<RawCmsgHdr>(remaining, 0)?;
        // `WSA_CMSG_DATA`: the payload starts at the aligned end of the header,
        // while `cmsg_len` counts the header plus the *unpadded* payload
        let data_start = cmsg_align(header_len);
        let data_end = hdr.len;
        // Reject a length that is too small to include the header, that implies
        // padding beyond the record, or that runs past the buffer. Any of these
        // means the buffer is not a well-formed cmsg stream, so iteration stops
        // entirely rather than trying to resynchronize.
        if hdr.len < header_len || data_start > data_end || data_end > remaining.len() {
            self.off = self.control.len();
            return None;
        }

        // The next record begins at the aligned end of this one. The `min`
        // prevents a bogus `cmsg_len` near `usize::MAX` from wrapping the offset
        // (`cmsg_align` rounds up) and re-reading the same bytes forever.
        self.off += cmsg_align(hdr.len).min(remaining.len());
        let data = remaining.get(data_start..data_end)?;
        Some(ControlMessage {
            level: hdr.level,
            ty: hdr.ty,
            data,
        })
    }
}

/// Aligns a control-message length the way `WSA_CMSG_ALIGN` does: to
/// `MAX_NATURAL_ALIGNMENT`, which is the platform pointer size (8 on 64-bit,
/// 4 on 32-bit).
///
/// `const` so it can be used in the compile-time capacity assertion in
/// `syscall::win`.
#[inline]
pub(crate) const fn cmsg_align(n: usize) -> usize {
    let a = align_of::<usize>();
    (n + a - 1) & !(a - 1)
}

#[inline]
pub(crate) fn read_u32(data: &[u8]) -> Option<u32> {
    read_at::<u32>(data, 0)
}

#[inline]
pub(crate) fn read_i32(data: &[u8]) -> Option<i32> {
    read_at::<i32>(data, 0)
}

/// Reads the leading `N` bytes of a control message payload, e.g. the address
/// at the start of an `IN_PKTINFO`/`IN6_PKTINFO`.
#[inline]
pub(crate) fn read_bytes<const N: usize>(data: &[u8]) -> Option<[u8; N]> {
    read_at::<[u8; N]>(data, 0)
}

#[inline]
fn read_at<T: Copy>(data: &[u8], off: usize) -> Option<T> {
    let end = off.checked_add(size_of::<T>())?;
    if end > data.len() {
        return None;
    }

    // SAFETY: Bounds are checked above. `read_unaligned` is used because
    // control buffers are byte slices and are not guaranteed to be aligned for
    // `T`, even though Winsock writes CMSG records into them.
    Some(unsafe { (data.as_ptr().add(off) as *const T).read_unaligned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPPROTO_IP: i32 = 0;
    const IPPROTO_UDP: i32 = 17;
    const IP_ECN: i32 = 50;
    const UDP_COALESCED_INFO: i32 = 3;

    fn cmsg(level: i32, ty: i32, data: &[u8]) -> Vec<u8> {
        let hdr_len = size_of::<RawCmsgHdr>();
        let len = hdr_len + data.len();
        let mut out = Vec::new();
        out.extend_from_slice(&len.to_ne_bytes());
        out.extend_from_slice(&level.to_ne_bytes());
        out.extend_from_slice(&ty.to_ne_bytes());
        out.resize(cmsg_align(hdr_len), 0);
        out.extend_from_slice(data);
        out.resize(cmsg_align(len), 0);
        out
    }

    #[test]
    fn iterates_multiple_control_messages() {
        let mut control = cmsg(IPPROTO_IP, IP_ECN, &3i32.to_ne_bytes());
        control.extend_from_slice(&cmsg(
            IPPROTO_UDP,
            UDP_COALESCED_INFO,
            &1200u32.to_ne_bytes(),
        ));

        let messages: Vec<_> = ControlMessages::new(&control).collect();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].level, IPPROTO_IP);
        assert_eq!(messages[0].ty, IP_ECN);
        assert_eq!(read_i32(messages[0].data), Some(3));
        assert_eq!(messages[1].level, IPPROTO_UDP);
        assert_eq!(messages[1].ty, UDP_COALESCED_INFO);
        assert_eq!(read_u32(messages[1].data), Some(1200));
    }

    #[test]
    fn ignores_truncated_header() {
        let control = vec![0u8; size_of::<RawCmsgHdr>() - 1];
        assert_eq!(ControlMessages::new(&control).count(), 0);
    }

    #[test]
    fn ignores_truncated_payload() {
        let mut control = cmsg(IPPROTO_IP, IP_ECN, &1i32.to_ne_bytes());
        control.truncate(size_of::<RawCmsgHdr>() + size_of::<i32>() - 1);
        assert_eq!(ControlMessages::new(&control).count(), 0);
    }

    #[test]
    fn ignores_invalid_zero_length() {
        let mut control = cmsg(IPPROTO_IP, IP_ECN, &1i32.to_ne_bytes());
        control[..size_of::<usize>()].copy_from_slice(&0usize.to_ne_bytes());
        assert_eq!(ControlMessages::new(&control).count(), 0);
    }

    #[test]
    fn ignores_length_that_wraps_alignment() {
        let mut control = cmsg(IPPROTO_IP, IP_ECN, &1i32.to_ne_bytes());
        control[..size_of::<usize>()].copy_from_slice(&(usize::MAX - 1).to_ne_bytes());
        assert_eq!(ControlMessages::new(&control).count(), 0);
    }
}

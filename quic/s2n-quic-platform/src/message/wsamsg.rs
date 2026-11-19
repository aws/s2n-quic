// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Platform [`Message`](MessageTrait) type for the Windows USO/URO IO path.
//!
//! This mirrors [`simple::Message`](crate::message::simple::Message) but additionally
//! carries the ECN marking and a per-datagram `segment_size`. The segment size
//! lets the RX path report URO (UDP receive coalescing) so a single
//! `WSARecvMsg` buffer is split back into individual datagrams by the platform's
//! [`RxMessage::for_each`].
//!
//! `SUPPORTS_GSO` is `true`: the platform `TxQueue` writes multiple packets into
//! a single entry payload and records the segment size with
//! [`set_segment_size`](MessageTrait::set_segment_size), which the socket task
//! turns into a `UDP_SEND_MSG_SIZE` control message (Windows USO).
//!
//! # Why this isn't `WSAMSG`
//!
//! On unix the message type *is* the syscall struct: `message::msg` implements
//! [`MessageTrait`](crate::message::Message) directly on `libc::msghdr`, and
//! `message::mmsg` on `libc::mmsghdr`, so the ring buffer memory can be handed
//! straight to `sendmmsg`/`recvmmsg` with no marshalling. That works because a
//! `msghdr` owns its `iovec`, name and control storage, and because the batching
//! syscalls consume an array of them.
//!
//! Winsock has no batching syscall, so nothing would consume an array of
//! `WSAMSG`. Keeping one per ring entry would only inflate the ring with the
//! name and control scratch buffers (~130B/entry) that a single in-flight call
//! doesn't need. Instead the entry stores just the decoded metadata, and
//! [`syscall::win`](crate::syscall::win) builds a throwaway `WSAMSG` on the
//! stack per call using scratch buffers owned by the socket task.
//!
//! A second consequence: on unix the segment size is stored by *encoding a
//! control message* into the entry's `msg_control` (`msghdr::set_segment_size`
//! calls `encode_gso`), whereas here `set_segment_size` just records an integer
//! that is serialized at send time. This is also why
//! `features::gso::IS_SUPPORTED` (a `UDP_SEGMENT` cmsg property) is `false` on
//! Windows even though segmentation offload works.

use crate::message::{Message as MessageTrait, RxMessage, Storage};
use core::alloc::Layout;
use s2n_quic_core::{
    inet::{datagram, ExplicitCongestionNotification, SocketAddress, Unspecified as _},
    io::tx,
    path,
};

/// Path handle type. Must match `s2n_quic_platform::message::default::Handle`
/// (i.e. `simple::Handle`) on Windows so `tokio-io`'s `PathHandle` stays
/// consistent when this message type is substituted into the ring buffers.
///
/// Unlike `msg::Handle`, which stores a raw `sockaddr` plus flow label so it can
/// be memcpy'd into a `msghdr`, `path::Tuple` is the portable representation:
/// nothing here is handed to the syscall verbatim, so there is no reason to
/// carry the platform layout around.
pub type Handle = path::Tuple;

/// A UDP datagram slot backed by the message ring buffer.
#[derive(Clone, Copy, Debug)]
pub struct Message {
    address: SocketAddress,
    /// The local address the datagram arrived on, as reported by the
    /// `IP_PKTINFO`/`IPV6_PKTINFO` control message.
    ///
    /// Unspecified when the stack didn't report one, in which case the socket's
    /// bound address is used instead. The port is always 0, since the control
    /// message doesn't carry one.
    local_address: SocketAddress,
    ecn: ExplicitCongestionNotification,
    /// The segment size of the payload, where 0 means "the whole payload is one
    /// datagram".
    ///
    /// On RX this is the URO coalescing size reported by Winsock. On TX it is the
    /// GSO segment size written by the platform `TxQueue` and is emitted as the
    /// USO `UDP_SEND_MSG_SIZE` control message.
    segment_size: usize,
    payload_ptr: *mut u8,
    payload_len: usize,
}

impl Message {
    #[inline]
    pub fn remote_address(&self) -> &SocketAddress {
        &self.address
    }

    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    /// The filled payload of the message.
    #[inline]
    pub fn payload(&self) -> &[u8] {
        // SAFETY: `payload_ptr`/`payload_len` describe a region within the ring
        // buffer storage allocated by `alloc`, and the length never exceeds the
        // originally allocated capacity.
        unsafe { core::slice::from_raw_parts(self.payload_ptr, self.payload_len) }
    }

    /// The USO segment size to send the payload with, if it holds more than one
    /// datagram.
    #[inline]
    pub fn segment_size(&self) -> Option<usize> {
        (self.segment_size > 0).then_some(self.segment_size)
    }

    /// Records the outcome of a receive into this message.
    ///
    /// `segment_size` is the URO coalescing size (or `len` when the datagram was
    /// not coalesced). `local` is the pktinfo address, if the stack reported one.
    #[inline]
    pub fn set_recv(
        &mut self,
        remote: SocketAddress,
        local: Option<SocketAddress>,
        ecn: ExplicitCongestionNotification,
        len: usize,
        segment_size: usize,
    ) {
        self.address = remote;
        self.local_address = local.unwrap_or_default();
        self.ecn = ecn;
        self.payload_len = len;
        self.segment_size = segment_size;
    }
}

impl MessageTrait for Message {
    type Handle = Handle;

    // USO availability is probed at runtime in `io::tokio`; this only says the
    // message type is *capable* of carrying multiple segments, which gates the
    // `TxQueue` packing logic. If the probe fails, `Gso::max_segments()` stays
    // at 1 and no segment size is ever recorded.
    const SUPPORTS_GSO: bool = true;
    const SUPPORTS_ECN: bool = true;
    // Windows has no `IPV6_FLOWINFO` send path equivalent to the unix
    // `msghdr` flow label support, so flow labels are not offered
    const SUPPORTS_FLOW_LABELS: bool = false;

    #[inline]
    fn alloc(entries: u32, payload_len: u32, offset: usize) -> Storage {
        unsafe { alloc(entries, payload_len, offset) }
    }

    #[inline]
    fn payload_len(&self) -> usize {
        self.payload_len
    }

    #[inline]
    unsafe fn set_payload_len(&mut self, len: usize) {
        self.payload_len = len;
    }

    #[inline]
    fn set_segment_size(&mut self, size: usize) {
        self.segment_size = size;
    }

    /// Returns the entry to the state expected by the next `recv`/`tx_write`.
    ///
    /// Much simpler than `msghdr::reset`, which has to zero and re-arm the
    /// control buffer so a stale `UDP_SEGMENT`/`IP_TOS` cmsg isn't resent. Here
    /// the metadata is plain fields, so clearing them is sufficient.
    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        self.payload_len = mtu;
        self.segment_size = 0;
        self.ecn = ExplicitCongestionNotification::default();
        self.address = SocketAddress::default();
        self.local_address = SocketAddress::default();
    }

    #[inline]
    fn payload_ptr_mut(&mut self) -> *mut u8 {
        self.payload_ptr
    }

    #[inline]
    fn validate_replication(source: &Self, dest: &Self) {
        assert_eq!(source.payload_ptr, dest.payload_ptr);
    }

    #[inline]
    fn rx_read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<RxMessage<'_, Self::Handle>> {
        let remote = self.address;
        let ecn = self.ecn;
        let seg = self.segment_size;

        // the control message only reports the IP, so the port comes from the
        // socket's bound address
        let local = if self.local_address.ip().is_unspecified() {
            *local_address
        } else {
            let mut local: path::LocalAddress = self.local_address.into();
            local.set_port(local_address.port());
            local
        };

        let payload = self.payload_mut();

        // A segment size of 0 means the datagram was not coalesced, so the whole
        // payload is a single segment. `min` guards against a stack reporting a
        // coalesced size larger than what was actually received, which would
        // make `RxMessage::for_each` produce a short/empty final segment.
        let segment_size = if seg == 0 {
            payload.len()
        } else {
            seg.min(payload.len())
        };

        let path = path::Tuple {
            remote_address: remote.into(),
            local_address: local,
        };
        let header = datagram::Header { path, ecn };

        Some(RxMessage {
            header,
            segment_size,
            payload,
        })
    }

    #[inline]
    fn tx_write<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<usize, tx::Error> {
        let payload = self.payload_mut();
        let capacity = payload.len();

        let len = message.write_payload(tx::PayloadBuffer::new(payload), 0)?;

        let len = len.min(capacity);
        unsafe {
            self.set_payload_len(len);
        }

        // A single datagram doesn't need a segment size; `TxQueue` calls
        // `set_segment_size` if more payloads are appended to this entry. It must
        // be cleared here because ring entries are reused and a stale value would
        // make the stack segment a single packet.
        self.segment_size = 0;
        self.address = message.path_handle().remote_address.0;
        self.ecn = message.ecn();

        Ok(len)
    }
}

/// Allocates `entries` message slots, each with a `payload_len` payload buffer.
///
/// The entry array is allocated at twice `entries` and the first half is copied
/// into the second. This is the same trick the unix message types use: the ring
/// buffer hands out contiguous slices that may wrap around the end, so the
/// mirrored copy makes a wrapped range look contiguous. Both copies point at the
/// *same* payload memory, which `validate_replication` asserts.
///
/// # Safety
/// The layout matches [`layout`] and the resulting `Storage` outlives every
/// message that points into it.
#[inline]
unsafe fn alloc(entries: u32, payload_len: u32, offset: usize) -> Storage {
    let (layout, entry_offset, payload_offset) = layout(entries, payload_len, offset);

    let storage = Storage::new(layout);

    // SAFETY: all pointer arithmetic stays within the freshly allocated
    // `storage` region (verified by `check_bounds`), and each entry is
    // initialized before being read.
    unsafe {
        let ptr = storage.as_ptr();

        let mut entry_ptr = ptr.add(entry_offset) as *mut Message;
        let mut payload_ptr = ptr.add(payload_offset);
        for _ in 0..entries {
            let entry = &mut *entry_ptr;
            entry.payload_ptr = payload_ptr;
            entry.payload_len = payload_len as _;

            entry_ptr = entry_ptr.add(1);
            payload_ptr = payload_ptr.add(payload_len as _);
            storage.check_bounds(entry_ptr);
            storage.check_bounds(payload_ptr);
        }

        let primary = ptr.add(entry_offset) as *mut Message;
        let secondary = primary.add(entries as _);
        storage.check_bounds(secondary.add(entries as _));
        core::ptr::copy_nonoverlapping(primary, secondary, entries as _);
    }

    storage
}

fn layout(entries: u32, payload_len: u32, offset: usize) -> (Layout, usize, usize) {
    let cursor = Layout::array::<u8>(offset).unwrap();
    let payloads = Layout::array::<u8>(entries as usize * payload_len as usize).unwrap();
    let entries = Layout::array::<Message>((entries * 2) as usize).unwrap();
    let (layout, entry_offset) = cursor.extend(entries).unwrap();
    let (layout, payload_offset) = layout.extend(payloads).unwrap();
    (layout, entry_offset, payload_offset)
}

#[cfg(test)]
#[allow(non_snake_case)] // tests are named `scenario__action__expectation`
mod tests {
    use super::*;

    const PAYLOAD_LEN: usize = 64;

    fn addr() -> SocketAddress {
        "127.0.0.1:4433"
            .parse::<std::net::SocketAddr>()
            .unwrap()
            .into()
    }

    fn handle() -> Handle {
        path::Tuple {
            remote_address: addr().into(),
            local_address: SocketAddress::default().into(),
        }
    }

    fn test_message<F: FnOnce(&mut Message)>(f: F) {
        let mut payload = [0u8; PAYLOAD_LEN];
        let mut message = Message {
            address: SocketAddress::default(),
            local_address: SocketAddress::default(),
            ecn: ExplicitCongestionNotification::default(),
            segment_size: 0,
            payload_ptr: payload.as_mut_ptr(),
            payload_len: PAYLOAD_LEN,
        };

        f(&mut message);
    }

    #[test]
    fn single_datagram__tx_write__reports_no_segment_size() {
        test_message(|message| {
            // a stale segment size from a previous send should not be reused
            message.segment_size = 8;

            let len = message
                .tx_write((handle(), ExplicitCongestionNotification::Ect0, [1u8; 16]))
                .unwrap();

            assert_eq!(len, 16);
            assert_eq!(message.payload_len(), 16);
            assert_eq!(message.segment_size(), None);
            assert_eq!(message.ecn(), ExplicitCongestionNotification::Ect0);
            assert_eq!(message.remote_address(), &addr());
        });
    }

    #[test]
    fn multiple_segments__set_segment_size__reports_segment_size() {
        test_message(|message| {
            message
                .tx_write((handle(), ExplicitCongestionNotification::Ect0, [1u8; 16]))
                .unwrap();

            // the platform `TxQueue` appends another segment and records the size
            unsafe {
                message.set_payload_len(32);
            }
            message.set_segment_size(16);

            assert_eq!(message.segment_size(), Some(16));
            assert_eq!(message.payload().len(), 32);
        });
    }

    #[test]
    fn coalesced_receive__reset__clears_segment_size() {
        test_message(|message| {
            message.set_recv(addr(), None, ExplicitCongestionNotification::Ce, 32, 16);
            assert_eq!(message.segment_size(), Some(16));

            unsafe {
                message.reset(PAYLOAD_LEN);
            }

            assert_eq!(message.segment_size(), None);
            assert_eq!(message.payload_len(), PAYLOAD_LEN);
        });
    }

    #[test]
    fn pktinfo_address__rx_read__reports_local_ip_with_socket_port() {
        let bound = "0.0.0.0:4433".parse::<std::net::SocketAddr>().unwrap();
        let bound: path::LocalAddress = SocketAddress::from(bound).into();
        // the control message reports the IP without a port
        let pktinfo = "192.168.0.1:0".parse::<std::net::SocketAddr>().unwrap();

        test_message(|message| {
            message.set_recv(
                addr(),
                Some(pktinfo.into()),
                ExplicitCongestionNotification::Ect0,
                16,
                0,
            );

            let rx = message.rx_read(&bound).unwrap();
            let local = rx.header.path.local_address;

            assert_eq!(local.ip(), SocketAddress::from(pktinfo).ip());
            assert_eq!(local.port(), 4433);
        });
    }

    #[test]
    fn no_pktinfo__rx_read__falls_back_to_bound_address() {
        let bound = "0.0.0.0:4433".parse::<std::net::SocketAddr>().unwrap();
        let bound: path::LocalAddress = SocketAddress::from(bound).into();

        test_message(|message| {
            message.set_recv(addr(), None, ExplicitCongestionNotification::Ect0, 16, 0);

            let rx = message.rx_read(&bound).unwrap();

            assert_eq!(rx.header.path.local_address, bound);
        });
    }
}

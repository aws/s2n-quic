// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{addr::Addr, cmsg};
use core::{fmt, task::Poll};
use libc::{msghdr, recvmsg};
use s2n_quic_core::{
    buffer::Deque as Buffer,
    ensure,
    inet::{ExplicitCongestionNotification, SocketAddress},
    ready,
};
use std::{io, os::fd::AsRawFd};

pub struct Message {
    addr: Addr,
    buffer: Buffer,
    recv: cmsg::Receiver,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Message")
            .field("addr", &self.addr)
            .field("segment_len", &self.recv.segment_len())
            .field("payload_len", &self.buffer.len())
            .field("ecn", &self.recv.ecn())
            .finish()
    }
}

impl Message {
    #[inline]
    pub fn new(max_mtu: u16) -> Self {
        let max_mtu = max_mtu as usize;
        let buffer_len = cmsg::MAX_GRO_SEGMENTS * max_mtu;
        // the recv syscall doesn't return more than this
        let buffer_len = buffer_len.min(u16::MAX as _);
        let buffer = Buffer::new(buffer_len);
        Self {
            addr: Addr::default(),
            buffer,
            recv: Default::default(),
        }
    }

    pub fn new_from_packet(bytes: Vec<u8>, addr: std::net::SocketAddr) -> Self {
        let buffer = Buffer::from(bytes);
        Self {
            addr: Addr::new(addr.into()),
            buffer,
            recv: Default::default(),
        }
    }

    #[inline]
    pub fn remote_address(&self) -> SocketAddress {
        self.addr.get()
    }

    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        self.recv.ecn()
    }

    #[inline]
    pub fn segments(&mut self) -> impl Iterator<Item = &mut [u8]> {
        self.buffer.make_contiguous();
        let len = self.buffer.len();
        let (head, _) = self.buffer.consume_filled(len).into();
        Segments::new(head, self.recv.take_segment_len())
    }

    #[inline]
    pub fn peek_segments(&mut self) -> impl Iterator<Item = &mut [u8]> {
        let bytes = self.buffer.make_contiguous();
        Segments::new(bytes, self.recv.segment_len())
    }

    #[inline]
    pub fn peek(&mut self) -> &mut [u8] {
        let segment_len = self.recv.segment_len() as usize;

        let (head, _tail) = self.buffer.filled().into();

        // if we have a segmented payload and the segment is larger than the head then make the
        // buffer contiguous
        if segment_len > 0 && segment_len > head.len() {
            self.buffer.make_contiguous();
        }

        let (head, _tail) = self.buffer.filled().into();

        if segment_len > 0 {
            let len = segment_len.min(head.len());
            &mut head[..len]
        } else {
            head
        }
    }

    #[inline]
    pub fn make_contiguous(&mut self) -> &mut [u8] {
        self.buffer.make_contiguous()
    }

    #[inline]
    pub fn consume(&mut self, len: usize) {
        self.buffer.consume(len);
    }

    #[inline]
    pub fn take(&mut self) -> Self {
        let capacity = self.buffer.capacity();
        core::mem::replace(
            self,
            Self {
                addr: Addr::default(),
                buffer: Buffer::new(capacity),
                recv: Default::default(),
            },
        )
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    #[inline]
    pub fn payload_len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn remaining_capacity(&self) -> usize {
        self.buffer.remaining_capacity()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.recv = Default::default();
    }

    #[inline]
    pub fn recv<S: AsRawFd>(&mut self, s: &S, flags: i32) -> io::Result<()> {
        self.clear();
        self.recv_impl(s, flags)
    }

    #[inline]
    pub fn recv_remaining<S: AsRawFd>(&mut self, s: &S, flags: i32) -> io::Result<()> {
        self.recv_impl(s, flags)
    }

    #[inline]
    pub fn recv_with<Recv>(&mut self, recv: Recv) -> io::Result<usize>
    where
        Recv: FnOnce(&mut Addr, &mut cmsg::Receiver, &mut [io::IoSliceMut]) -> io::Result<usize>,
    {
        let mut unfilled = unsafe {
            // SAFETY: only the recvmsg syscall writes to the segments
            self.buffer.unfilled().assume_init_io_slice_mut()
        };

        let len = recv(&mut self.addr, &mut self.recv, unfilled.reader_slice_mut())?;

        unsafe {
            // SAFETY: we mostly trust the OS to correctly report filled bytes
            self.buffer.fill(len)?;
        }

        Ok(len)
    }

    #[inline]
    pub fn poll_recv_with<Recv>(&mut self, recv: Recv) -> Poll<io::Result<usize>>
    where
        Recv: FnOnce(
            &mut Addr,
            &mut cmsg::Receiver,
            &mut [io::IoSliceMut],
        ) -> Poll<io::Result<usize>>,
    {
        let mut unfilled = unsafe {
            // SAFETY: only the recvmsg syscall writes to the segments
            self.buffer.unfilled().assume_init_io_slice_mut()
        };

        let len = ready!(recv(
            &mut self.addr,
            &mut self.recv,
            unfilled.reader_slice_mut()
        ))?;

        unsafe {
            // SAFETY: we mostly trust the OS to correctly report filled bytes
            self.buffer.fill(len)?;
        }

        Ok(len).into()
    }

    #[inline]
    fn recv_impl<S: AsRawFd>(&mut self, s: &S, flags: i32) -> io::Result<()> {
        self.recv_with(|addr, cmsg, iov| {
            let mut msg = unsafe { core::mem::zeroed::<msghdr>() };

            msg.msg_iov = iov.as_mut_ptr() as *mut _;
            msg.msg_iovlen = iov.len() as _;

            addr.recv_with_msg(&mut msg);

            let mut cmsg_storage = cmsg::Storage::<{ cmsg::DECODER_LEN }>::default();
            msg.msg_control = cmsg_storage.as_mut_ptr() as *mut _;
            msg.msg_controllen = cmsg_storage.len() as _;

            let result = unsafe { recvmsg(s.as_raw_fd(), &mut msg, flags as _) };

            addr.update_with_msg(&msg);

            ensure!(result > 0, Err(io::Error::last_os_error()));

            let len = result as usize;

            cmsg.with_msg(&msg);

            Ok(len)
        })?;

        Ok(())
    }

    #[inline]
    pub fn test_recv(
        &mut self,
        remote_addr: SocketAddress,
        ecn: ExplicitCongestionNotification,
        payload: Vec<u8>,
    ) {
        debug_assert!(self.is_empty());
        self.addr.set(remote_addr);
        self.recv.set_ecn(ecn);
        self.recv
            .set_segment_len(payload.len().try_into().expect("payload too large"));
        self.buffer = payload.into();
    }
}

pub struct Segments<'a> {
    buffer: &'a mut [u8],
    segment_len: usize,
}

impl<'a> Segments<'a> {
    #[inline]
    fn new(buffer: &'a mut [u8], segment_len: u16) -> Self {
        let segment_len = if segment_len == 0 {
            buffer.len()
        } else {
            segment_len as _
        };
        Self {
            buffer,
            segment_len,
        }
    }
}

impl<'a> Iterator for Segments<'a> {
    type Item = &'a mut [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let len = self.buffer.len().min(self.segment_len);
        ensure!(len > 0, None);
        let (head, tail) = self.buffer.split_at_mut(len);
        let (head, tail) = unsafe {
            // SAFETY: we're just extending the lifetime of this split off segment
            core::mem::transmute::<(&mut [u8], &mut [u8]), (&mut [u8], &mut [u8])>((head, tail))
        };
        self.buffer = tail;
        Some(head)
    }
}

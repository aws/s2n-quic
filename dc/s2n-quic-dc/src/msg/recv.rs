// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{addr::Addr, cmsg};
use core::fmt;
use libc::{iovec, msghdr, recvmsg};
use s2n_quic_core::{
    assume, branch, ensure,
    inet::{ExplicitCongestionNotification, SocketAddress},
    path::MaxMtu,
};
use std::{io, os::fd::AsRawFd};
use tracing::trace;

pub struct Message {
    addr: Addr,
    buffer: Vec<u8>,
    recv: cmsg::Receiver,
    payload_len: usize,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Message")
            .field("addr", &self.addr)
            .field("segment_len", &self.recv.segment_len())
            .field("payload_len", &self.payload_len)
            .field("ecn", &self.recv.ecn())
            .finish()
    }
}

impl Message {
    #[inline]
    pub fn new(max_mtu: MaxMtu) -> Self {
        let max_mtu: u16 = max_mtu.into();
        let max_mtu = max_mtu as usize;
        let buffer_len = cmsg::MAX_GRO_SEGMENTS * max_mtu;
        // the recv syscall doesn't return more than this
        let buffer_len = buffer_len.min(u16::MAX as _);
        let buffer = Vec::with_capacity(buffer_len);
        Self {
            addr: Addr::default(),
            buffer,
            recv: Default::default(),
            payload_len: 0,
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
        let payload_len = core::mem::replace(&mut self.payload_len, 0);
        Segments {
            buffer: &mut self.buffer[..payload_len],
            segment_len: self.recv.take_segment_len(),
        }
    }

    #[inline]
    pub fn peek_segments(&mut self) -> impl Iterator<Item = &mut [u8]> {
        Segments {
            buffer: &mut self.buffer[..self.payload_len],
            segment_len: self.recv.segment_len(),
        }
    }

    #[inline]
    pub fn take(&mut self) -> Self {
        let capacity = self.buffer.capacity();
        core::mem::replace(
            self,
            Self {
                addr: Addr::default(),
                buffer: Vec::with_capacity(capacity),
                recv: Default::default(),
                payload_len: 0,
            },
        )
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.segment_len() == 0
    }

    #[inline]
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.payload_len = 0;
        self.recv.set_segment_len(0);
    }

    #[inline]
    pub fn recv<S: AsRawFd>(&mut self, s: &S) -> io::Result<()> {
        let mut msg = unsafe { core::mem::zeroed::<msghdr>() };

        let mut iovec = unsafe { core::mem::zeroed::<iovec>() };

        iovec.iov_base = self.buffer.as_mut_ptr() as *mut _;
        iovec.iov_len = self.buffer.capacity() as _;

        msg.msg_iov = &mut iovec;
        msg.msg_iovlen = 1;

        self.addr.recv_with_msg(&mut msg);

        let mut cmsg = cmsg::Storage::<{ cmsg::DECODER_LEN }>::default();
        msg.msg_control = cmsg.as_mut_ptr() as *mut _;
        msg.msg_controllen = cmsg.len() as _;

        let flags = Default::default();

        let result = unsafe { recvmsg(s.as_raw_fd(), &mut msg, flags) };

        self.addr.update_with_msg(&msg);

        trace!(
            src = %self.addr,
            cmsg_len = msg.msg_controllen,
            result,
        );

        if !branch!(result > 0) {
            let error = io::Error::last_os_error();

            if !matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) {
                tracing::error!(?error);
            }

            return Err(error);
        }

        let len = result as usize;

        unsafe {
            assume!(self.buffer.capacity() >= len);
            self.buffer.set_len(len);
        }

        self.payload_len = len;
        self.recv.with_msg(&msg, len);

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
        self.payload_len = payload.len();
        self.buffer = payload;
    }
}

pub struct Segments<'a> {
    buffer: &'a mut [u8],
    segment_len: u16,
}

impl<'a> Iterator for Segments<'a> {
    type Item = &'a mut [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let len = self.buffer.len().min(self.segment_len as _);
        ensure!(len > 0, None);
        let (head, tail) = self.buffer.split_at_mut(len);
        let (head, tail) = unsafe {
            // SAFETY: we're just extending the lifetime of this split off segment
            core::mem::transmute((head, tail))
        };
        self.buffer = tail;
        Some(head)
    }
}

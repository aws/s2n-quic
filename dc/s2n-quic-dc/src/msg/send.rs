// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{addr::Addr, cmsg};
use crate::allocator::{self, Allocator};
use core::{fmt, num::NonZeroU16, task::Poll};
use libc::{iovec, msghdr, sendmsg};
use s2n_quic_core::{
    assume, ensure,
    inet::{ExplicitCongestionNotification, SocketAddress, Unspecified},
    ready,
};
use s2n_quic_platform::features;
use std::{io, os::fd::AsRawFd};
use tracing::trace;

type Idx = u16;
type RetransmissionIdx = NonZeroU16;

#[cfg(debug_assertions)]
type Instance = u64;
#[cfg(not(debug_assertions))]
type Instance = ();

#[inline(always)]
fn instance_id() -> Instance {
    #[cfg(debug_assertions)]
    {
        use core::sync::atomic::{AtomicU64, Ordering};
        static INSTANCES: AtomicU64 = AtomicU64::new(0);
        INSTANCES.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct Segment {
    idx: Idx,
    instance_id: Instance,
}

impl Segment {
    #[inline(always)]
    fn get<'a>(&'a self, buffers: &'a [Vec<u8>]) -> &'a Vec<u8> {
        unsafe {
            assume!(buffers.len() > self.idx as usize);
        }
        &buffers[self.idx as usize]
    }

    #[inline(always)]
    fn get_mut<'a>(&self, buffers: &'a mut [Vec<u8>]) -> &'a mut Vec<u8> {
        unsafe {
            assume!(buffers.len() > self.idx as usize);
        }
        &mut buffers[self.idx as usize]
    }
}

impl allocator::Segment for Segment {
    #[inline]
    fn leak(&mut self) {
        self.idx = Idx::MAX;
    }
}

#[cfg(debug_assertions)]
impl Drop for Segment {
    fn drop(&mut self) {
        if self.idx != Idx::MAX && !std::thread::panicking() {
            panic!("message segment {} leaked", self.idx);
        }
    }
}

#[derive(Debug)]
pub struct Retransmission {
    idx: RetransmissionIdx,
    instance_id: Instance,
}

impl allocator::Segment for Retransmission {
    #[inline]
    fn leak(&mut self) {
        self.idx = unsafe { RetransmissionIdx::new_unchecked(Idx::MAX) };
    }
}

impl Retransmission {
    #[inline(always)]
    fn idx(&self) -> Idx {
        self.idx.get() - 1
    }

    #[inline(always)]
    fn get<'a>(&'a self, buffers: &'a [Vec<u8>]) -> &'a Vec<u8> {
        let idx = self.idx() as usize;
        unsafe {
            assume!(buffers.len() > idx);
        }
        &buffers[idx]
    }

    #[inline]
    fn into_segment(mut self) -> Segment {
        let idx = core::mem::replace(&mut self.idx, unsafe {
            RetransmissionIdx::new_unchecked(Idx::MAX)
        });
        let idx = idx.get() - 1;
        let instance_id = self.instance_id;
        Segment { idx, instance_id }
    }

    #[inline]
    fn from_segment(mut handle: Segment) -> Self {
        let idx = core::mem::replace(&mut handle.idx, Idx::MAX);
        let idx = idx.saturating_add(1);
        let idx = unsafe { RetransmissionIdx::new_unchecked(idx) };
        let instance_id = handle.instance_id;
        Retransmission { idx, instance_id }
    }
}

#[cfg(debug_assertions)]
impl Drop for Retransmission {
    fn drop(&mut self) {
        if self.idx.get() != Idx::MAX && !std::thread::panicking() {
            panic!("message segment {} leaked", self.idx.get());
        }
    }
}

pub struct Message {
    addr: Addr,
    gso: features::Gso,
    segment_len: u16,
    total_len: u16,
    can_push: bool,
    buffers: Vec<Vec<u8>>,
    free: Vec<Segment>,
    pending_free: Vec<Segment>,
    payload: Vec<libc::iovec>,
    ecn: ExplicitCongestionNotification,
    instance_id: Instance,
    #[cfg(debug_assertions)]
    allocated: std::collections::BTreeSet<Idx>,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut d = f.debug_struct("Message");

        d.field("addr", &self.addr)
            .field("segment_len", &self.segment_len)
            .field("total_len", &self.total_len)
            .field("can_push", &self.can_push)
            .field("buffers", &self.buffers.len())
            .field("free", &self.free.len())
            .field("pending_free", &self.pending_free.len())
            .field("segments", &self.payload.len())
            .field("ecn", &self.ecn);

        #[cfg(debug_assertions)]
        {
            d.field("instance_id", &self.instance_id)
                .field("allocated", &self.allocated.len());
        }

        d.finish()
    }
}

unsafe impl Send for Message {}
unsafe impl Sync for Message {}

impl Message {
    #[inline]
    pub fn new(remote_address: SocketAddress, gso: features::Gso) -> Self {
        let burst_size = 16;
        Self {
            addr: Addr::new(remote_address),
            gso,
            segment_len: 0,
            total_len: 0,
            can_push: true,
            buffers: Vec::with_capacity(burst_size),
            free: Vec::with_capacity(burst_size),
            pending_free: Vec::with_capacity(burst_size),
            payload: Vec::with_capacity(burst_size),
            ecn: ExplicitCongestionNotification::NotEct,
            instance_id: instance_id(),
            #[cfg(debug_assertions)]
            allocated: Default::default(),
        }
    }

    #[inline]
    fn push_payload(&mut self, segment: &Segment) {
        debug_assert!(self.can_push());
        debug_assert_eq!(segment.instance_id, self.instance_id);

        let mut iovec = unsafe { core::mem::zeroed::<iovec>() };
        let buffer = segment.get_mut(&mut self.buffers);

        debug_assert!(!buffer.is_empty());
        debug_assert!(
            buffer.len() <= u16::MAX as usize,
            "cannot transmit more than 2^16 bytes in a single packet"
        );

        let iov_base: *mut u8 = buffer.as_mut_ptr();
        iovec.iov_base = iov_base as *mut _;
        iovec.iov_len = buffer.len() as _;

        self.total_len += buffer.len() as u16;

        if self.payload.is_empty() {
            self.segment_len = buffer.len() as _;
        } else {
            debug_assert!(buffer.len() <= self.segment_len as usize);
            // the caller can only push until the last undersized segment
            self.can_push &= buffer.len() == self.segment_len as usize;
        }

        self.payload.push(iovec);

        let max_segments = self.gso.max_segments();

        self.can_push &= self.payload.len() < max_segments;

        // sendmsg has a limitation on the total length of the payload, even with GSO
        let next_size = self.total_len as usize + self.segment_len as usize;
        let max_size = u16::MAX as usize;
        self.can_push &= next_size <= max_size;
    }

    #[inline]
    pub fn send_with<Snd>(&mut self, send: Snd) -> io::Result<usize>
    where
        Snd: FnOnce(&Addr, ExplicitCongestionNotification, &[io::IoSlice]) -> io::Result<usize>,
    {
        let iov = unsafe {
            // SAFETY: IoSlice is guaranteed to have the same layout as iovec
            &*(self.payload.as_slice() as *const [libc::iovec] as *const [io::IoSlice])
        };
        let res = send(&self.addr, self.ecn, iov);
        self.on_transmit(&res);
        res
    }

    #[inline]
    pub fn poll_send_with<Snd>(&mut self, send: Snd) -> Poll<io::Result<usize>>
    where
        Snd: FnOnce(
            &Addr,
            ExplicitCongestionNotification,
            &[io::IoSlice],
        ) -> Poll<io::Result<usize>>,
    {
        let iov = unsafe {
            // SAFETY: IoSlice is guaranteed to have the same layout as iovec
            &*(self.payload.as_slice() as *const [libc::iovec] as *const [io::IoSlice])
        };
        let res = ready!(send(&self.addr, self.ecn, iov));
        self.on_transmit(&res);
        res.into()
    }

    #[inline]
    pub fn send<S: AsRawFd>(&mut self, s: &S) -> io::Result<()> {
        let segment_len = self.segment_len;

        self.send_with(|addr, ecn, iov| {
            use cmsg::Encoder as _;

            let mut msg = unsafe { core::mem::zeroed::<msghdr>() };

            msg.msg_iov = iov.as_ptr() as *mut _;
            msg.msg_iovlen = iov.len() as _;

            debug_assert!(
                !addr.get().ip().is_unspecified(),
                "cannot send packet to unspecified address"
            );
            debug_assert!(
                addr.get().port() != 0,
                "cannot send packet to unspecified port"
            );
            addr.send_with_msg(&mut msg);

            let mut cmsg_storage = cmsg::Storage::<{ cmsg::ENCODER_LEN }>::default();
            let mut cmsg = cmsg_storage.encoder();
            if ecn != ExplicitCongestionNotification::NotEct {
                // TODO enable this once we consolidate s2n-quic-core crates
                // let _ = cmsg.encode_ecn(ecn, &addr);
            }

            if iov.len() > 1 {
                let _ = cmsg.encode_gso(segment_len);
            }

            if !cmsg.is_empty() {
                msg.msg_control = cmsg.as_mut_ptr() as *mut _;
                msg.msg_controllen = cmsg.len() as _;
            }

            let flags = Default::default();

            let result = unsafe { sendmsg(s.as_raw_fd(), &msg, flags) };

            trace!(
                dest = %addr,
                segments = iov.len(),
                segment_len,
                cmsg_len = msg.msg_controllen,
                result,
            );

            if result >= 0 {
                Ok(result as usize)
            } else {
                Err(io::Error::last_os_error())
            }
        })?;

        Ok(())
    }

    #[inline]
    pub fn drain(&mut self) -> Drain {
        Drain {
            message: self,
            index: 0,
        }
    }

    /// The maximum number of segments that can be sent in a single GSO payload
    #[inline]
    pub fn max_segments(&self) -> usize {
        self.gso.max_segments()
    }

    #[inline]
    fn on_transmit(&mut self, result: &io::Result<usize>) {
        let len = match result {
            Ok(len) => *len,
            Err(err) => {
                // notify the GSO impl that we got an error
                self.gso.handle_socket_error(err);
                return;
            }
        };

        if self.total_len as usize > len {
            todo!();
        }

        self.force_clear()
    }
}

impl Allocator for Message {
    type Segment = Segment;

    type Retransmission = Retransmission;

    #[inline]
    fn alloc(&mut self) -> Option<Self::Segment> {
        ensure!(self.can_push(), None);

        if let Some(segment) = self.free.pop() {
            #[cfg(debug_assertions)]
            assert!(self.allocated.insert(segment.idx));
            trace!(operation = "alloc", ?segment);
            return Some(segment);
        }

        let idx = self.buffers.len().try_into().ok()?;
        let instance_id = self.instance_id;
        let segment = Segment { idx, instance_id };
        self.buffers.push(vec![]);

        #[cfg(debug_assertions)]
        assert!(self.allocated.insert(segment.idx));
        trace!(operation = "alloc", ?segment);

        Some(segment)
    }

    #[inline]
    fn get<'a>(&'a self, segment: &'a Segment) -> &'a Vec<u8> {
        debug_assert_eq!(segment.instance_id, self.instance_id);

        #[cfg(debug_assertions)]
        assert!(self.allocated.contains(&segment.idx));

        segment.get(&self.buffers)
    }

    #[inline]
    fn get_mut(&mut self, segment: &Segment) -> &mut Vec<u8> {
        debug_assert_eq!(segment.instance_id, self.instance_id);

        #[cfg(debug_assertions)]
        assert!(self.allocated.contains(&segment.idx));

        segment.get_mut(&mut self.buffers)
    }

    #[inline]
    fn push(&mut self, segment: Segment) {
        trace!(operation = "push", ?segment);
        self.push_payload(&segment);

        #[cfg(debug_assertions)]
        assert!(self.allocated.contains(&segment.idx));

        self.pending_free.push(segment);
    }

    #[inline]
    fn push_with_retransmission(&mut self, segment: Segment) -> Retransmission {
        trace!(operation = "push_with_retransmission", ?segment);
        self.push_payload(&segment);

        #[cfg(debug_assertions)]
        assert!(self.allocated.contains(&segment.idx));

        Retransmission::from_segment(segment)
    }

    #[inline]
    fn retransmit(&mut self, segment: Retransmission) -> Segment {
        debug_assert_eq!(segment.instance_id, self.instance_id);
        debug_assert!(
            self.payload.is_empty(),
            "cannot retransmit with pending payload"
        );

        let segment = segment.into_segment();

        #[cfg(debug_assertions)]
        assert!(self.allocated.contains(&segment.idx));

        segment
    }

    #[inline]
    fn retransmit_copy(&mut self, retransmission: &Retransmission) -> Option<Segment> {
        debug_assert_eq!(retransmission.instance_id, self.instance_id);
        #[cfg(debug_assertions)]
        assert!(
            self.allocated.contains(&retransmission.idx()),
            "{retransmission:?} {self:?}"
        );

        let segment = self.alloc()?;

        let mut target = core::mem::take(self.get_mut(&segment));
        debug_assert!(target.is_empty());

        let source = retransmission.get(&self.buffers);
        debug_assert!(
            !source.is_empty(),
            "cannot retransmit empty payload; source: {retransmission:?}, target: {segment:?}"
        );
        target.extend_from_slice(source);

        *self.get_mut(&segment) = target;

        Some(segment)
    }

    #[inline]
    fn can_push(&self) -> bool {
        self.can_push
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    #[inline]
    fn segment_len(&self) -> Option<u16> {
        debug_assert_eq!(self.segment_len == 0, self.is_empty());
        if self.segment_len == 0 {
            None
        } else {
            Some(self.segment_len)
        }
    }

    #[inline]
    fn free(&mut self, segment: Segment) {
        debug_assert_eq!(segment.instance_id, self.instance_id);
        trace!(operation = "free", ?segment);

        #[cfg(debug_assertions)]
        assert!(self.allocated.contains(&segment.idx));

        // if we haven't actually sent anything then immediately free it
        if self.is_empty() {
            #[cfg(debug_assertions)]
            assert!(self.allocated.remove(&segment.idx));

            self.free.push(segment);
        } else {
            self.pending_free.push(segment);
        }
    }

    #[inline]
    fn free_retransmission(&mut self, segment: Retransmission) {
        debug_assert_eq!(segment.instance_id, self.instance_id);
        debug_assert!(
            self.payload.is_empty(),
            "cannot free a retransmission with pending payload"
        );

        trace!(operation = "free_retransmission", ?segment);

        let segment = segment.into_segment();

        let buffer = self.get_mut(&segment);
        buffer.clear();

        #[cfg(debug_assertions)]
        assert!(self.allocated.remove(&segment.idx));

        self.free.push(segment);
    }

    #[inline]
    fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    #[inline]
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
        self.ecn = ecn;
    }

    #[inline]
    fn remote_address(&self) -> SocketAddress {
        self.addr.get()
    }

    #[inline]
    fn set_remote_address(&mut self, remote_address: SocketAddress) {
        self.addr.set(remote_address);
    }

    #[inline]
    fn set_remote_port(&mut self, port: u16) {
        self.addr.set_port(port);
    }

    #[inline]
    fn force_clear(&mut self) {
        // reset the current payload
        self.payload.clear();
        self.ecn = ExplicitCongestionNotification::NotEct;
        self.segment_len = 0;
        self.total_len = 0;
        self.can_push = true;

        for segment in &self.pending_free {
            segment.get_mut(&mut self.buffers).clear();
            #[cfg(debug_assertions)]
            assert!(self.allocated.remove(&segment.idx));
        }

        if self.free.is_empty() {
            core::mem::swap(&mut self.free, &mut self.pending_free);
        } else {
            self.free.append(&mut self.pending_free);
        }
    }
}

#[cfg(debug_assertions)]
impl Drop for Message {
    fn drop(&mut self) {
        use allocator::Segment;
        for segment in &mut self.free {
            segment.leak();
        }
        for segment in &mut self.pending_free {
            segment.leak();
        }
    }
}

pub struct Drain<'a> {
    message: &'a mut Message,
    index: usize,
}

impl<'a> Iterator for Drain<'a> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let v = self.message.payload.get(self.index)?;
        self.index += 1;
        let v = unsafe { core::slice::from_raw_parts(v.iov_base as *const u8, v.iov_len) };
        Some(v)
    }
}

impl<'a> Drop for Drain<'a> {
    #[inline]
    fn drop(&mut self) {
        self.message.force_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();

        let addr: std::net::SocketAddr = "127.0.0.1:4433".parse().unwrap();
        let mut message = Message::new(addr.into(), Default::default());

        let handle = message.alloc().unwrap();
        let payload = message.get_mut(&handle);
        payload.extend_from_slice(b"hello\n");
        let hello = message.push_with_retransmission(handle);

        let world = if message.gso.max_segments() > 1 {
            let handle = message.alloc().unwrap();
            let payload = message.get_mut(&handle);
            payload.extend_from_slice(b"world\n");
            let world = message.push_with_retransmission(handle);
            Some(world)
        } else {
            None
        };

        message.send(&socket).unwrap();

        let world = world.map(|world| message.retransmit(world));
        let hello = message.retransmit(hello);

        if let Some(world) = world {
            assert_eq!(message.get(&world), b"world\n");
            message.push(world);
        }

        assert_eq!(message.get(&hello), b"hello\n");
        message.push(hello);

        message.send(&socket).unwrap();
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{Buffer as _, VecBuffer},
    io::thread_socket::ThreadSocket,
    message::{mmsg::Ring as MmsgRing, Message as MessageTrait},
};
use core::{
    cell::UnsafeCell,
    task::{Context, Poll},
};
use s2n_quic_core::{
    io::tx,
    sync::ring_lock::{Consumer, Producer, Region},
};
use std::{net::UdpSocket, os::unix::io::AsRawFd, sync::Arc};

pub struct ConcurrentTx {
    socket: ThreadSocket,
    producer: Producer,
    region: Region,
    buffer: Arc<UnsafeCell<SharedBuffer>>,
    needs_poll: bool,
}

unsafe impl Send for ConcurrentTx {}

struct SharedBuffer {
    messages: Vec<Message>,

    ring: MmsgRing<VecBuffer>,
}

impl SharedBuffer {
    pub fn new() -> Self {
        let mut ring = MmsgRing::new(VecBuffer::default());

        let mut messages = vec![];

        let len = ring.payloads.len();
        for i in 0..len {
            messages.push(Message {
                primary: ring.messages[i].as_mut_ptr() as *mut _,
                seconary: ring.messages[i + len].as_mut_ptr() as *mut _,
            });
        }

        Self { messages, ring }
    }
}

impl ConcurrentTx {
    pub fn new(socket: UdpSocket) -> Self {
        let buffer = SharedBuffer::new();
        let (mut producer, consumer) =
            s2n_quic_core::sync::ring_lock::new(buffer.ring.payloads.len());
        let buffer = UnsafeCell::new(buffer);
        let buffer = Arc::new(buffer);
        let socket = MmmgSocket {
            consumer,
            socket,
            buffer: buffer.clone(),
            stats: None,
        };
        let socket = ThreadSocket::new(socket);
        let region = producer.current_region();
        Self {
            socket,
            buffer,
            producer,
            region,
            needs_poll: region.len == 0,
        }
    }

    pub async fn writable(&mut self) {
        // only poll the socket when we hit the capacity on push
        if self.needs_poll {
            self.region = futures::future::poll_fn(|cx| self.producer.poll_region(cx)).await;
            self.needs_poll = false;
        } else {
            futures::future::pending().await
        }
    }

    pub fn tx_queue(&mut self) -> Queue {
        // update the region view
        self.region = self.producer.current_region();
        Queue {
            tx: self,
            pushed_len: 0,
        }
    }
}

pub struct Queue<'a> {
    tx: &'a mut ConcurrentTx,
    pushed_len: usize,
}

pub struct Message {
    primary: *mut crate::message::mmsg::Message,
    seconary: *mut crate::message::mmsg::Message,
}

impl tx::Entry for Message {
    fn set<M: tx::Message>(&mut self, message: M) -> Result<usize, tx::Error> {
        let primary = unsafe { &mut *self.primary };
        let len = primary.set(message)?;

        unsafe { &mut *self.seconary }.replicate_fields_from(primary);

        Ok(len)
    }

    fn payload(&self) -> &[u8] {
        MessageTrait::payload(unsafe { &*self.primary })
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        MessageTrait::payload_mut(unsafe { &mut *self.primary })
    }
}

impl<'a> tx::Queue for Queue<'a> {
    type Entry = Message;

    fn push<M: tx::Message>(&mut self, message: M) -> Result<usize, tx::Error> {
        use tx::Entry;

        if self.pushed_len == self.tx.region.len {
            self.tx.needs_poll = true;
            return Err(tx::Error::AtCapacity);
        }

        let buffer = &mut unsafe { &mut *self.tx.buffer.get() }.messages;

        let index = self.tx.region.index + self.pushed_len;
        let index = index % buffer.len();
        unsafe { buffer.get_unchecked_mut(index) }.set(message)?;
        self.pushed_len += 1;

        Ok(index)
    }

    fn len(&self) -> usize {
        todo!()
    }

    fn capacity(&self) -> usize {
        todo!()
    }

    fn as_slice_mut(&mut self) -> &mut [Self::Entry] {
        todo!()
    }
}

impl<'a> Drop for Queue<'a> {
    fn drop(&mut self) {
        if self.pushed_len > 0 {
            unsafe {
                // Safety: we check that the pushed_len does not exceed the
                //         acquired len
                self.tx.producer.push_unchecked(self.pushed_len);
                self.tx.region.index += self.pushed_len;
                self.tx.region.len -= self.pushed_len;
                if self.tx.region.len == 0 {
                    self.tx.needs_poll = true;
                }
            }
        }
    }
}

pub struct MmmgSocket {
    consumer: Consumer,
    socket: UdpSocket,
    buffer: Arc<UnsafeCell<SharedBuffer>>,
    stats: Option<Stats>,
}

struct Stats {
    packets: u64,
    bytes: u64,
    time: std::time::Instant,
    loops: u64,
}

impl Stats {
    fn new() -> Self {
        Self {
            packets: 0,
            bytes: 0,
            time: std::time::Instant::now(),
            loops: 0,
        }
    }

    fn mbps(&self) -> u64 {
        (self.bytes as f64 / self.time.elapsed().as_secs_f64() / 1_000_000.0 * 8.) as u64
    }

    fn pps(&self) -> u64 {
        (self.packets as f64 / self.time.elapsed().as_secs_f64()) as u64
    }

    fn on_send(&mut self, bytes: usize) {
        self.bytes += bytes as u64;
        self.packets += 1;
    }
}

unsafe impl Send for MmmgSocket {}

impl crate::io::thread_socket::Socket for MmmgSocket {
    fn poll_progress(&mut self, cx: &mut Context<'_>) -> crate::io::thread_socket::Control {
        use crate::io::thread_socket::Control;

        let region = if let Poll::Ready(region) = self.consumer.poll_region(cx) {
            region
        } else {
            if let Some(stats) = self.stats.take() {
                if stats.packets > 10 {
                    eprintln!("\nMbps: {}", stats.mbps());
                    eprintln!("PPS: {}", stats.pps());
                }
            }
            return Control::Sleep;
        };

        let stats = if let Some(stats) = self.stats.as_mut() {
            stats
        } else {
            self.stats = Some(Stats::new());
            self.stats.as_mut().unwrap()
        };

        stats.loops += 1;

        if stats.loops == 10_000 {
            eprintln!("\nMbps: {}", stats.mbps());
            eprintln!("PPS: {}", stats.pps());
        }

        unsafe {
            let buffer = &mut *self.buffer.get();

            let entries = &mut buffer.ring.messages[region.index..region.index + region.len];

            // Safety: calling a libc function is inherently unsafe as rust cannot
            // make any invariant guarantees. This has to be reviewed by humans instead
            // so the [docs](https://linux.die.net/man/2/sendmmsg) are inlined here:

            // > The sockfd argument is the file descriptor of the socket on which data
            // > is to be transmitted.
            let sockfd = self.socket.as_raw_fd();

            // > The msgvec argument is a pointer to an array of mmsghdr structures.
            //
            // > The msg_hdr field is a msghdr structure, as described in sendmsg(2).
            // > The msg_len field is used to return the number of bytes sent from the
            // > message in msg_hdr.
            let msgvec = entries.as_mut_ptr() as _;

            // > The size of this array is specified in vlen.
            //
            // > The value specified in vlen is capped to UIO_MAXIOV (1024).
            let vlen = entries.len() as _;

            // > The flags argument contains flags ORed together.
            //
            // No flags are currently set
            let flags = Default::default();

            // > The sendmmsg() system call is an extension of sendmsg(2) that allows
            // > the caller to transmit multiple messages on a socket using a single
            // > system call. (This has performance benefits for some applications.)
            //
            // > A nonblocking call sends as many messages as possible (up to the limit
            // > specified by vlen) and returns immediately.
            //
            // > On return from sendmmsg(), the msg_len fields of successive elements
            // > of msgvec are updated to contain the number of bytes transmitted from
            // > the corresponding msg_hdr. The return value of the call indicates the
            // > number of elements of msgvec that have been updated.
            //
            // > On success, sendmmsg() returns the number of messages sent from msgvec;
            // > if this is less than vlen, the caller can retry with a further sendmmsg()
            // > call to send the remaining messages.
            //
            // > On error, -1 is returned, and errno is set to indicate the error.
            match libc::sendmmsg(sockfd, msgvec, vlen, flags) {
                status if status >= 0 => {
                    let count = status as usize;

                    let count = count.min(1024);

                    self.consumer.pop_unchecked(count);

                    for entry in &mut entries[..count] {
                        stats.on_send(entry.payload_len());
                        entry.set_payload_len(buffer.ring.payloads.mtu());
                    }

                    Control::Continue
                }
                _ => {
                    let err = std::io::Error::last_os_error();

                    if err.kind() == std::io::ErrorKind::Interrupted {
                        return Control::Continue;
                    }

                    Control::Break
                }
            }
        }
    }
}

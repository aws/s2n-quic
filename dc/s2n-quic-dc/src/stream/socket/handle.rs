// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Protocol, TransportFeatures};
use crate::msg::{self, addr::Addr, cmsg};
use core::task::{Context, Poll};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice, IoSliceMut},
    net::SocketAddr,
    sync::Arc,
};

pub type Flags = libc::c_int;

pub trait Socket: 'static + Send + Sync {
    /// Returns the local address for the socket
    fn local_addr(&self) -> io::Result<SocketAddr>;

    /// Returns the local port for the socket
    #[inline]
    fn local_port(&self) -> io::Result<u16> {
        Ok(self.local_addr()?.port())
    }

    fn protocol(&self) -> Protocol;

    /// Returns the [`TransportFeatures`] that the socket supports
    fn features(&self) -> TransportFeatures;

    /// Returns the amount of buffered data on the socket
    fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>>;

    #[inline]
    fn poll_recv_buffer(
        &self,
        cx: &mut Context,
        msg: &mut msg::recv::Message,
    ) -> Poll<io::Result<usize>> {
        #[cfg(debug_assertions)]
        if !self.features().is_stream() {
            assert!(
                msg.is_empty(),
                "receive buffer should be empty for datagram protocols"
            );
        }

        msg.poll_recv_with(|addr, cmsg, buffer| self.poll_recv(cx, addr, cmsg, buffer))
    }

    /// Receives data on the socket
    fn poll_recv(
        &self,
        cx: &mut Context,
        addr: &mut Addr,
        cmsg: &mut cmsg::Receiver,
        buffer: &mut [IoSliceMut],
    ) -> Poll<io::Result<usize>>;

    #[inline]
    fn try_send_buffer(&self, msg: &mut msg::send::Message) -> io::Result<usize> {
        msg.send_with(|addr, ecn, iov| self.try_send(addr, ecn, iov))
    }

    /// Tries to send data on the socket, returning `Err(WouldBlock)` if none could be sent.
    fn try_send(
        &self,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> io::Result<usize>;

    #[inline]
    fn poll_send_buffer(
        &self,
        cx: &mut Context,
        msg: &mut msg::send::Message,
    ) -> Poll<io::Result<usize>> {
        msg.poll_send_with(|addr, ecn, iov| self.poll_send(cx, addr, ecn, iov))
    }

    /// Sends data on the socket
    fn poll_send(
        &self,
        cx: &mut Context,
        addr: &Addr,
        ecn: ExplicitCongestionNotification,
        buffer: &[IoSlice],
    ) -> Poll<io::Result<usize>>;

    /// Shuts down the sender half of the socket, if a concept exists
    fn send_finish(&self) -> io::Result<()>;
}

pub trait Ext: Socket {
    #[inline]
    fn recv_buffer<'a>(&'a self, msg: &'a mut msg::recv::Message) -> ExtRecvBuffer<'a, Self> {
        ExtRecvBuffer { socket: self, msg }
    }
}

pub struct ExtRecvBuffer<'a, T: Socket + ?Sized> {
    socket: &'a T,
    msg: &'a mut msg::recv::Message,
}

impl<'a, T: Socket> core::future::Future for ExtRecvBuffer<'a, T> {
    type Output = io::Result<usize>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.socket.poll_recv_buffer(cx, self.msg)
    }
}

impl<T: Socket> Ext for T {}

macro_rules! impl_box {
    ($b:ident) => {
        impl<T: Socket> Socket for $b<T> {
            #[inline(always)]
            fn local_addr(&self) -> io::Result<SocketAddr> {
                (**self).local_addr()
            }

            #[inline(always)]
            fn protocol(&self) -> Protocol {
                (**self).protocol()
            }

            #[inline(always)]
            fn features(&self) -> TransportFeatures {
                (**self).features()
            }

            #[inline(always)]
            fn poll_peek_len(&self, cx: &mut Context) -> Poll<io::Result<usize>> {
                (**self).poll_peek_len(cx)
            }

            #[inline(always)]
            fn poll_recv(
                &self,
                cx: &mut Context,
                addr: &mut Addr,
                cmsg: &mut cmsg::Receiver,
                buffer: &mut [IoSliceMut],
            ) -> Poll<io::Result<usize>> {
                (**self).poll_recv(cx, addr, cmsg, buffer)
            }

            #[inline(always)]
            fn try_send(
                &self,
                addr: &Addr,
                ecn: ExplicitCongestionNotification,
                buffer: &[IoSlice],
            ) -> io::Result<usize> {
                (**self).try_send(addr, ecn, buffer)
            }

            #[inline(always)]
            fn poll_send(
                &self,
                cx: &mut Context,
                addr: &Addr,
                ecn: ExplicitCongestionNotification,
                buffer: &[IoSlice],
            ) -> Poll<io::Result<usize>> {
                (**self).poll_send(cx, addr, ecn, buffer)
            }

            #[inline(always)]
            fn send_finish(&self) -> io::Result<()> {
                (**self).send_finish()
            }
        }
    };
}

impl_box!(Box);
impl_box!(Arc);

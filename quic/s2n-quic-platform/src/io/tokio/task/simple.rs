// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    features::Gso,
    message::{simple::Message, Message as _},
    socket::{
        ring, stats, task,
        task::{rx, tx},
    },
    syscall::SocketEvents,
};
use core::task::{Context, Poll};
use s2n_quic_core::task::cooldown::Cooldown;
use tokio::{io, net::UdpSocket};

pub async fn rx<S: Into<std::net::UdpSocket>>(
    socket: S,
    producer: ring::Producer<Message>,
    cooldown: Cooldown,
    stats: stats::Sender,
) -> io::Result<()> {
    let socket = socket.into();
    socket.set_nonblocking(true).unwrap();

    let socket = UdpSocket::from_std(socket).unwrap();
    let result = task::Receiver::new(producer, socket, cooldown, stats).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

pub async fn tx<S: Into<std::net::UdpSocket>>(
    socket: S,
    consumer: ring::Consumer<Message>,
    gso: Gso,
    cooldown: Cooldown,
    stats: stats::Sender,
) -> io::Result<()> {
    let socket = socket.into();
    socket.set_nonblocking(true).unwrap();

    let socket = UdpSocket::from_std(socket).unwrap();
    let result = task::Sender::new(consumer, socket, gso, cooldown, stats).await;
    if let Some(err) = result {
        Err(err)
    } else {
        Ok(())
    }
}

impl tx::Socket<Message> for UdpSocket {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        cx: &mut Context,
        entries: &mut [Message],
        events: &mut tx::Events,
        stats: &stats::Sender,
    ) -> io::Result<()> {
        for entry in entries {
            let target = (*entry.remote_address()).into();
            let payload = entry.payload_mut();

            let res = self.poll_send_to(cx, payload, target);
            stats.send().on_operation(&res, |_len| 1);
            match res {
                Poll::Ready(Ok(_)) => {
                    if events.on_complete(1).is_break() {
                        return Ok(());
                    }
                }
                Poll::Ready(Err(err)) => {
                    if events.on_error(err).is_break() {
                        return Ok(());
                    }
                }
                Poll::Pending => {
                    events.blocked();
                    break;
                }
            }
        }

        Ok(())
    }
}

impl rx::Socket<Message> for UdpSocket {
    type Error = io::Error;

    #[inline]
    fn recv(
        &mut self,
        cx: &mut Context,
        entries: &mut [Message],
        events: &mut rx::Events,
        stats: &stats::Sender,
    ) -> io::Result<()> {
        for entry in entries {
            let payload = entry.payload_mut();
            let mut buf = io::ReadBuf::new(payload);

            let res = self.poll_recv_from(cx, &mut buf);
            stats.recv().on_operation(&res, |_len| 1);
            match res {
                Poll::Ready(Ok(addr)) => {
                    unsafe {
                        let len = buf.filled().len();
                        entry.set_payload_len(len);
                    }
                    entry.set_remote_address(&(addr.into()));

                    if events.on_complete(1).is_break() {
                        return Ok(());
                    }
                }
                Poll::Ready(Err(err)) => {
                    if events.on_error(err).is_break() {
                        return Ok(());
                    }
                }
                Poll::Pending => {
                    events.blocked();
                    break;
                }
            }
        }

        Ok(())
    }
}

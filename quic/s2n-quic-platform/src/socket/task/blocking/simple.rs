// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    message::{simple::Message, Message as _},
    socket::task::{rx, tx},
    syscall::SocketEvents,
};
use core::task::Context;
use std::{io, net::UdpSocket};

impl tx::Socket<Message> for UdpSocket {
    type Error = io::Error;

    #[inline]
    fn send(
        &mut self,
        _cx: &mut Context,
        entries: &mut [Message],
        events: &mut tx::Events,
    ) -> io::Result<()> {
        let mut index = 0;

        while let Some(entry) = entries.get_mut(index) {
            let target = *entry.remote_address();
            let payload = entry.payload_mut();
            match self.send_to(payload, target) {
                Ok(_) => {
                    index += 1;
                    if events.on_complete(1).is_break() {
                        return Ok(());
                    }
                }
                Err(err) => {
                    if events.on_error(err).is_break() {
                        return Ok(());
                    }
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
        _cx: &mut Context,
        entries: &mut [Message],
        events: &mut rx::Events,
    ) -> io::Result<()> {
        if let Some(entry) = entries.first_mut() {
            let payload = entry.payload_mut();
            match self.recv_from(payload) {
                Ok((len, addr)) => {
                    unsafe {
                        entry.set_payload_len(len);
                    }
                    entry.set_remote_address(&(addr.into()));

                    if events.on_complete(1).is_break() {
                        return Ok(());
                    }
                }
                Err(err) => {
                    if events.on_error(err).is_break() {
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }
}

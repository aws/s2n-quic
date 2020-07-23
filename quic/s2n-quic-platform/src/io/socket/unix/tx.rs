use crate::io::{
    buffer::message::MessageBuffer,
    socket::unix::{queue::MessageQueue, udp::UdpSocket},
    tx::{TxError, TxPayload, TxQueue},
};
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};
use std::{io, os::unix::io::AsRawFd};

const FLAGS: i32 = 0;

#[derive(Debug)]
pub struct TxBuffer<Buffer>(MessageQueue<Buffer>);

impl<Buffer: MessageBuffer> TxBuffer<Buffer> {
    pub fn new(buffer: Buffer) -> Self {
        TxBuffer(MessageQueue::new(buffer))
    }

    #[cfg(s2n_quic_platform_socket_mmsg)]
    pub(crate) fn sync(&mut self, socket: &mut UdpSocket) -> io::Result<usize> {
        let pending = self.0.pending_mut();

        if pending.is_empty() {
            return Ok(0);
        }

        unsafe {
            match libc::sendmmsg(
                socket.as_raw_fd(),
                pending.as_ptr() as _,
                pending.len() as u32,
                FLAGS,
            ) {
                status if status < 0 => {
                    pending.cancel();
                    Err(io::Error::last_os_error())
                }
                count => {
                    let count = count as usize;

                    pending.finish(count);

                    Ok(count)
                }
            }
        }
    }

    #[cfg(not(s2n_quic_platform_socket_mmsg))]
    pub(crate) fn sync(&mut self, socket: &mut UdpSocket) -> io::Result<usize> {
        use super::udp::UdpSocketExt;

        let mut count = 0;

        while let Some((msg, msg_cursor)) = self.0.pop_pending() {
            if count == 1 {
                socket.enable_nonblocking()?;
            }

            unsafe {
                match libc::sendmsg(socket.as_raw_fd(), msg.as_ptr() as _, FLAGS) {
                    status if status < 0 => {
                        msg_cursor.cancel();
                        let err = io::Error::last_os_error();

                        if err.kind() == io::ErrorKind::WouldBlock && count > 0 {
                            socket.reset_nonblocking()?;
                            break;
                        }

                        return Err(err);
                    }
                    _ => {
                        msg_cursor.finish();
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }
}

impl<Buffer: MessageBuffer> TxQueue for TxBuffer<Buffer> {
    fn push<Payload: TxPayload>(
        &mut self,
        remote_address: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        payload: Payload,
    ) -> Result<usize, TxError> {
        let max_payload_size = self.0.max_payload_size();
        let (mut msg, msg_cursor) = if let Some(res) = self.0.pop_ready() {
            res
        } else {
            return Err(TxError::AtCapacity);
        };

        // Set the buffer size to the maximum payload size
        msg.set_payload_len(max_payload_size);

        match payload.write(msg.payload_mut()) {
            0 => {
                msg_cursor.cancel();

                Err(TxError::Cancelled)
            }
            payload_len => {
                msg.set_remote_address(remote_address);
                msg.set_ecn(ecn);
                msg.set_payload_len(payload_len);

                msg_cursor.finish();

                Ok(payload_len)
            }
        }
    }

    fn capacity(&self) -> usize {
        self.0.ready_len()
    }

    fn len(&self) -> usize {
        self.0.pending_len()
    }
}

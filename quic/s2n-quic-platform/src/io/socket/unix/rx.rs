use crate::{
    buffer::Buffer as MessageBuffer,
    io::{
        rx::RxQueue,
        socket::unix::{
            queue::{new as new_queue, MessageQueue},
            udp::UdpSocket,
        },
    },
    message::Message,
};
use s2n_quic_core::{inet::DatagramInfo, time::Timestamp};
use std::{io, os::unix::io::AsRawFd};

const FLAGS: i32 = 0;

#[derive(Debug)]
pub struct RxBuffer<Buffer: MessageBuffer>(MessageQueue<Buffer>);

impl<Buffer: MessageBuffer> RxBuffer<Buffer> {
    pub fn new(buffer: Buffer) -> Self {
        RxBuffer(new_queue(buffer))
    }

    #[cfg(s2n_quic_platform_socket_mmsg)]
    pub(crate) fn sync(&mut self, socket: &mut UdpSocket) -> io::Result<usize> {
        let mut ready = self.0.ready_mut();

        if ready.is_empty() {
            return Ok(0);
        }

        unsafe {
            match libc::recvmmsg(
                socket.as_raw_fd(),
                ready.as_mut_ptr() as _,
                ready.len() as u32,
                FLAGS,
                core::ptr::null_mut(),
            ) {
                status if status < 0 => {
                    ready.cancel();
                    Err(io::Error::last_os_error())
                }
                count => {
                    let count = count as usize;

                    ready.finish(count);

                    Ok(count)
                }
            }
        }
    }

    #[cfg(not(s2n_quic_platform_socket_mmsg))]
    pub(crate) fn sync(&mut self, socket: &mut UdpSocket) -> io::Result<usize> {
        use super::udp::UdpSocketExt;

        let mut count = 0;

        while let Some((mut msg, cursor)) = self.0.pop_ready() {
            if count == 1 {
                socket.enable_nonblocking()?;
            }

            unsafe {
                match libc::recvmsg(socket.as_raw_fd(), msg.as_mut_ptr() as _, FLAGS) {
                    status if status < 0 => {
                        cursor.cancel();
                        let err = io::Error::last_os_error();

                        if err.kind() == io::ErrorKind::WouldBlock && count > 0 {
                            socket.reset_nonblocking()?;
                            break;
                        }

                        return Err(err);
                    }
                    len => {
                        msg.set_payload_len(len as usize);

                        cursor.finish();

                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }
}

impl<Buffer: MessageBuffer> RxQueue for RxBuffer<Buffer> {
    fn pop(&mut self, timestamp: Timestamp) -> Option<(DatagramInfo, &mut [u8])> {
        let mtu = self.0.mtu();
        let (mut msg, cursor) = self.0.pop_pending()?;

        let remote_address = msg.remote_address().unwrap_or_default();
        let ecn = msg.ecn();

        // Reset the msg back to the max address size
        msg.reset_remote_address();

        // Reset the msg back to the maximum size.
        let payload_len = msg.payload_len();
        unsafe {
            msg.set_payload_len(mtu);
        }

        // return the payload down
        let payload = msg.payload_ptr_mut();

        // Safety: the current buffer is locked until this payload goes out of scope
        let payload = unsafe { core::slice::from_raw_parts_mut(payload, payload_len) };

        // move ownership back to the ready pool
        cursor.finish();

        let info = DatagramInfo {
            timestamp,
            remote_address,
            ecn,
            payload_len,
        };

        Some((info, payload))
    }

    fn len(&self) -> usize {
        self.0.pending_len()
    }
}

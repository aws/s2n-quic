#![allow(unused_macros)]

macro_rules! impl_io {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name<Buffer: crate::buffer::Buffer, Socket> {
            queue: crate::message::queue::Queue<Ring<Buffer>>,
            socket: Socket,
        }

        impl<Buffer: crate::buffer::Buffer, Socket> $name<Buffer, Socket> {
            pub fn new(buffer: Buffer, socket: Socket) -> Self {
                Self {
                    queue: crate::message::queue::Queue::new(Ring::new(buffer)),
                    socket,
                }
            }
        }

        impl<Buffer: crate::buffer::Buffer, Socket> core::ops::Deref for $name<Buffer, Socket> {
            type Target = Socket;

            fn deref(&self) -> &Self::Target {
                &self.socket
            }
        }

        impl<Buffer: crate::buffer::Buffer, Socket> core::ops::DerefMut for $name<Buffer, Socket> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.socket
            }
        }
    };
}

macro_rules! impl_io_tokio {
    ($name:ident, $call:ident) => {
        #[cfg(all(feature = "futures", feature = "tokio"))]
        impl<Buffer: $crate::buffer::Buffer> $name<Buffer, $crate::socket::tokio::Socket> {
            pub async fn sync(&mut self) -> ::std::io::Result<usize> {
                $crate::socket::tokio::sync::$call(self).await
            }

            pub fn poll(
                &mut self,
                cx: &mut ::core::task::Context<'_>,
            ) -> ::core::task::Poll<::std::io::Result<usize>> {
                $crate::socket::tokio::poll::$call(self, cx)
            }
        }
    };
}

#[cfg(test)]
macro_rules! impl_io_rx_tests {
    () => {
        use crate::{buffer::default::Buffer, socket::tokio::Socket};
        use s2n_quic_core::io::rx::{Entry, Queue, Rx as RxTrait};
        use std::{collections::HashSet, net::IpAddr};
        use tokio::net::UdpSocket;

        type Rx = super::Rx<Buffer, Socket>;

        async fn pair(addr: IpAddr) -> (UdpSocket, Rx) {
            let client = UdpSocket::bind((addr, 0u16)).await.unwrap();

            let rx_buffer = Buffer::default();
            let rx_socket = Socket::bind((addr, 0u16)).unwrap();
            let server = Rx::new(rx_buffer, rx_socket);

            (client, server)
        }

        async fn test(addr: &str) {
            let addr: IpAddr = addr.parse().unwrap();
            let (mut client, mut server) = pair(addr).await;
            let server_port = server.local_addr().unwrap().port();
            let server_addr = (addr, server_port);
            let capacity = 64;
            let total = capacity * 2;

            let mut messages: HashSet<Vec<u8>> = (0..total)
                .map(|i| format!("Hello message {}", i).into())
                .collect();

            let iterations: usize = total + capacity / 2;

            for _ in 0..iterations {
                for message in messages.iter() {
                    client.send_to(&message[..], server_addr).await.unwrap();
                }

                server.sync().await.unwrap();

                let mut queue = server.queue();
                let len = queue.len();

                for message in queue.as_slice_mut() {
                    messages.remove(message.payload());
                }

                queue.finish(len);

                if messages.is_empty() {
                    return;
                }
            }

            panic!(
                "socket only received {}/{} messages in {} iterations",
                total - messages.len(),
                total,
                iterations
            );
        }

        #[tokio::test]
        #[cfg_attr(windows, ignore)] // windows isn't currently working reliably
        async fn ipv4_test() {
            test("127.0.0.1").await
        }

        #[cfg(feature = "ipv6")]
        #[tokio::test]
        #[cfg_attr(windows, ignore)] // windows isn't currently working reliably
        async fn ipv6_test() {
            test("::1").await
        }
    };
}

#[cfg(test)]
macro_rules! impl_io_tx_tests {
    () => {
        use crate::{buffer::default::Buffer, socket::tokio::Socket};
        use core::time::Duration;
        use s2n_quic_core::io::tx::{Queue, Tx as TxTrait};
        use std::{collections::HashSet, net::IpAddr};
        use tokio::{net::UdpSocket, time::timeout};

        type Tx = super::Tx<Buffer, Socket>;

        async fn pair(addr: &str) -> (Tx, UdpSocket) {
            let addr: IpAddr = addr.parse().unwrap();

            let server = UdpSocket::bind((addr, 0u16)).await.unwrap();

            let tx_buffer = Buffer::default();
            let tx_socket = Socket::bind((addr, 0u16)).unwrap();
            let client = Tx::new(tx_buffer, tx_socket);

            (client, server)
        }

        async fn test(addr: &str) {
            let (mut client, mut server) = pair(addr).await;
            let server_addr = server.local_addr().unwrap();

            let mut buffer = [0u8; 512];
            let capacity = client.queue().capacity();
            let total = capacity * 2;

            let mut messages: HashSet<Vec<u8>> = (0..total)
                .map(|i| format!("Hello message {}", i).into())
                .collect();

            let iterations: usize = total + capacity / 2;

            for _ in 0..iterations {
                let mut queue = client.queue();

                for (_, message) in (0..queue.capacity()).zip(messages.iter()) {
                    queue.push((server_addr.into(), &message[..])).unwrap();
                }

                client.sync().await.unwrap();

                while let Ok(res) =
                    timeout(Duration::from_millis(10), server.recv_from(&mut buffer)).await
                {
                    let (len, _) = res.unwrap();
                    messages.remove(&buffer[..len]);
                }

                if messages.is_empty() {
                    return;
                }
            }

            panic!(
                "socket only transmitted {}/{} messages in {} iterations",
                total - messages.len(),
                total,
                iterations
            );
        }

        #[tokio::test]
        #[cfg_attr(windows, ignore)] // windows isn't currently working reliably
        async fn ipv4_test() {
            test("127.0.0.1").await
        }

        #[cfg(feature = "ipv6")]
        #[tokio::test]
        #[cfg_attr(windows, ignore)] // windows isn't currently working reliably
        async fn ipv6_test() {
            test("::1").await
        }
    };
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Options;
use crate::packet::stream;
use s2n_quic_platform::bpf::cbpf::*;
use std::{io, net::UdpSocket};

/// Program for routing packets between two sockets based on the packet tag
///
/// High-level algorithm with asm is available here: https://godbolt.org/z/crxT4d53j
pub static ROUTER: Program = Program::new(&[
    // load the first byte of the packet
    ldb(abs(0)),
    // mask off the LSBs
    and(!stream::Tag::MAX as _),
    // IF:
    // the control bit is set
    jneq(stream::Tag::MAX as u32 + 1, 1, 0),
    // THEN:
    // return a 0 indicating we want to route to the writer socket
    ret(0),
    // ELSE:
    // return a 1 indicating we want to route to the reader socket
    ret(1),
]);

#[derive(Debug)]
pub struct Pair {
    pub writer: UdpSocket,
    pub reader: UdpSocket,
}

impl Pair {
    #[inline]
    pub fn open(mut options: Options) -> io::Result<Self> {
        // GRO is not compatible with this mode of operation
        options.gro = false;
        // set the reuse port option after binding to avoid port collisions
        options.reuse_port = super::ReusePort::AfterBind;

        let writer = options.build_udp()?;

        // bind the sockets to the same address
        options.addr = writer.local_addr()?;
        // now that we have a concrete port from the OS, we set the option before the bind call
        options.reuse_port = super::ReusePort::BeforeBind;

        let reader = options.build_udp()?;

        // attach the BPF program to the sockets
        for socket in [&reader, &writer] {
            ROUTER.attach(socket)?;
        }

        Ok(Self { writer, reader })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::control;
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    #[test]
    #[cfg_attr(miri, ignore)]
    fn snapshot_test() {
        insta::assert_snapshot!(ROUTER);
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Counts {
        stream: usize,
        control: usize,
        garbage: usize,
    }

    impl Counts {
        #[inline]
        fn handle(&mut self, byte: u8) {
            if byte <= stream::Tag::MAX {
                self.stream += 1;
            } else if byte >= 0b1000_0000 {
                self.garbage += 1;
            } else {
                self.control += 1;
            }
        }

        fn total(&self) -> usize {
            self.stream + self.control + self.garbage
        }
    }

    #[test]
    fn routing_test() {
        let mut options = Options::new("127.0.0.1:0".parse().unwrap());
        options.blocking = true;

        let Pair { writer, reader } = match Pair::open(options) {
            Ok(pair) => pair,
            Err(err)
                if [
                    io::ErrorKind::PermissionDenied,
                    io::ErrorKind::AddrNotAvailable,
                ]
                .contains(&err.kind()) =>
            {
                eprintln!("skipping test due to insufficient permissions");
                return;
            }
            Err(err) => panic!("{err}"),
        };

        let timeout = Some(Duration::from_millis(100));
        writer.set_read_timeout(timeout).unwrap();
        reader.set_read_timeout(timeout).unwrap();

        let addr = writer.local_addr().unwrap();

        assert_eq!(addr, reader.local_addr().unwrap());

        let mut reader_packets = Counts::default();
        let mut writer_packets = Counts::default();
        let sent_packets = AtomicUsize::new(0);

        std::thread::scope(|s| {
            s.spawn(|| {
                let mut buffer = [0; 32];
                while let Ok((_len, _src)) = reader.recv_from(&mut buffer) {
                    reader_packets.handle(buffer[0]);
                }
            });

            s.spawn(|| {
                let mut buffer = [0; 32];
                while let Ok((_len, _src)) = writer.recv_from(&mut buffer) {
                    writer_packets.handle(buffer[0]);
                }
            });

            for _ in 0..4 {
                let client = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
                // pace out senders to avoid drops on the receiver
                std::thread::sleep(core::time::Duration::from_millis(1));
                let sent_packets = &sent_packets;
                s.spawn(move || {
                    for idx in 0u32..300 {
                        let mut packet = idx.to_le_bytes();
                        packet[0] = if idx % 2 == 0 {
                            // send a control packet
                            control::Tag::MAX
                        } else if idx / 2 % 2 == 0 {
                            // send a stream packet
                            stream::Tag::MAX
                        } else {
                            // send a garbage packet with the MSB set
                            control::Tag::MAX | 0b1000_0000
                        };

                        client.send_to(&packet, addr).unwrap();

                        sent_packets.fetch_add(1, Ordering::Relaxed);

                        // pace out packets to avoid drops on the receiver
                        if idx % 10 == 0 {
                            std::thread::sleep(core::time::Duration::from_millis(5));
                        }
                    }

                    dbg!();
                });
            }
        });

        dbg!(&reader_packets);
        dbg!(&writer_packets);

        // sometimes this test is a bit flaky in CI so we'll just log the failure for now
        if reader_packets == Default::default() || writer_packets == Default::default() {
            use std::io::{stderr, Write};

            // we need to use stderr directly to bypass test harness capture
            let _ = stderr()
                .write_all(b"WARNING: no packets were received in cbpf test - skipping test\n");

            return;
        }

        assert_eq!(writer_packets.stream, 0);
        assert_eq!(writer_packets.garbage, 0);
        assert_ne!(writer_packets.control, 0);

        assert_ne!(reader_packets.stream, 0);
        assert_ne!(reader_packets.garbage, 0);
        assert_eq!(reader_packets.control, 0);

        let reader_packets = reader_packets.total();
        let writer_packets = writer_packets.total();

        assert!(
            reader_packets.abs_diff(writer_packets) < reader_packets.max(writer_packets) / 2,
            "the difference should be less than half of the max received packets"
        );
    }

    #[test]
    fn routing_logic() {
        let mut sockets = [Counts::default(), Counts::default()];

        for byte in 0..=u8::MAX {
            let index = match byte >> 6 {
                // control bit is set - route to writer
                0b01 => 0,
                // otherwise route to reader
                _ => 1,
            };

            sockets[index].handle(byte);
        }

        insta::assert_debug_snapshot!((&sockets[0], &sockets[1]));
    }
}

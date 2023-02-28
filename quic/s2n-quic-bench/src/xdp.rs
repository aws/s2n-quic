// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, IpV4Address, IpV6Address},
    io::tx::{self, PayloadBuffer},
    xdp::{
        encoder::{encode_packet, State},
        path,
    },
};

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("xdp/encoder");
    let overhead = 100;

    let paths = [
        (
            "ipv4",
            true,
            path::Tuple {
                remote_address: path::RemoteAddress {
                    mac: Default::default(),
                    ip: IpV4Address::default().into(),
                    port: 0,
                },
                local_address: path::LocalAddress {
                    mac: Default::default(),
                    ip: IpV4Address::default().into(),
                    port: 0,
                },
            },
        ),
        (
            "ipv4-no-checksum",
            false,
            path::Tuple {
                remote_address: path::RemoteAddress {
                    mac: Default::default(),
                    ip: IpV4Address::default().into(),
                    port: 0,
                },
                local_address: path::LocalAddress {
                    mac: Default::default(),
                    ip: IpV4Address::default().into(),
                    port: 0,
                },
            },
        ),
        (
            "ipv6",
            true,
            path::Tuple {
                remote_address: path::RemoteAddress {
                    mac: Default::default(),
                    ip: IpV6Address::default().into(),
                    port: 0,
                },
                local_address: path::LocalAddress {
                    mac: Default::default(),
                    ip: IpV6Address::default().into(),
                    port: 0,
                },
            },
        ),
    ];

    for (label, ipv4_checksum, path) in paths {
        for payload_len in [1500, 9000, 1 << 16] {
            let message = Message {
                path,
                ecn: Default::default(),
                ipv6_flow_label: 123,
                payload_len: payload_len - overhead,
            };

            group.throughput(Throughput::Elements(1));
            group.bench_with_input(
                BenchmarkId::new(label, payload_len),
                &message,
                |b, mut message| {
                    let mut buffer = vec![0u8; payload_len];
                    let mut state = State::default();
                    state.set_checksum(ipv4_checksum);

                    b.iter(|| {
                        let mut encoder = EncoderBuffer::new(&mut buffer);
                        let _ = black_box(encode_packet(
                            black_box(&mut encoder),
                            black_box(&mut message),
                            black_box(&mut state),
                        ));
                    })
                },
            );
        }
    }
    group.finish();
}

#[derive(Debug)]
struct Message {
    path: path::Tuple,
    ecn: ExplicitCongestionNotification,
    ipv6_flow_label: u32,
    payload_len: usize,
}

impl<'a> tx::Message for &'a Message {
    type Handle = path::Tuple;

    fn path_handle(&self) -> &Self::Handle {
        &self.path
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        self.ecn
    }

    fn delay(&mut self) -> core::time::Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        self.ipv6_flow_label
    }

    fn can_gso(&self, _: usize, _: usize) -> bool {
        true
    }

    fn write_payload(
        &mut self,
        _buffer: PayloadBuffer,
        _gso_offset: usize,
    ) -> Result<usize, tx::Error> {
        // skip copying the payload to just measure the header overhead
        Ok(self.payload_len)
    }
}

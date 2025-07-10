// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{BenchmarkId, Criterion};
use std::hint::black_box;

const PACKET: [u8; 90] = [
    64, 0, 42, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 4, 0, 55, 67, 102, 47, 62, 183, 50, 8, 44,
    222, 220, 128, 156, 98, 0, 128, 201, 9, 228, 4, 62, 25, 149, 52, 227, 53, 226, 10, 143, 72, 79,
    180, 16, 46, 173, 156, 16, 215, 240, 248, 7, 147, 159, 101, 36, 161, 156, 117, 188, 75, 88,
    125, 182, 220, 74, 234, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

macro_rules! impl_recv {
    ($name:ident) => {
        mod $name {
            use s2n_quic_dc::packet::datagram::Tag;
            // use s2n_quic_dc::datagram::send::send as send_impl;
            // pub use s2n_quic_dc::datagram::send::testing::$name::{state, State};

            /*
            #[inline(never)]
            pub fn recv(state: &mut State, mut input: &[u8]) {
                let _ = send_impl(state, &mut (), &mut input);
            }
            */
            #[inline(never)]
            #[allow(dead_code)]
            pub fn parse(
                buffer: &mut [u8],
            ) -> Option<s2n_quic_dc::packet::datagram::decoder::Packet> {
                let buffer = s2n_codec::DecoderBufferMut::new(buffer);
                let (packet, _buffer) = s2n_quic_dc::packet::datagram::decoder::Packet::decode(
                    buffer,
                    Tag::default(),
                    16,
                )
                .ok()?;
                Some(packet)
            }
        }
    };
}

impl_recv!(null);
impl_recv!(aes_128_gcm);
impl_recv!(aes_256_gcm);

#[allow(const_item_mutation)]
pub fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("datagram/recv");

    // let input = black_box(&mut [1u8, 2, 3][..]);

    group.bench_with_input(BenchmarkId::new("test", 1), &(), |b, _input| {
        b.iter(move || {
            let _ = black_box(null::parse(black_box(&mut PACKET[..])));
        });
    });

    /*

    let headers = [0, 16];

    let payloads = [
        1, //1, 100, 1000, 1450,
        8900,
    ];

    let inline = [
        ("aes_128_gcm", &aws_lc_rs::aead::AES_128_GCM),
        ("aes_256_gcm", &aws_lc_rs::aead::AES_256_GCM),
    ];

    for payload_size in payloads {
        let payload = black_box(vec![42u8; payload_size]);
        for header_size in headers {
            let header = black_box(vec![42u8; header_size]);

            group.throughput(Throughput::Elements(1));

            let input_name = format!("payload={payload_size},header={header_size}");

            macro_rules! bench {
                ($name:ident) => {{
                    let id = BenchmarkId::new(stringify!($name), &input_name);

                    if header_size > 0 {
                        group.bench_with_input(
                            id,
                            &(&header[..], &payload[..]),
                            |b, (header, payload)| {
                                let mut state = black_box($name::state(creds(42).next().unwrap()));
                                b.iter(move || {
                                    let _ =
                                        black_box($name::send_header(&mut state, header, payload));
                                });
                            },
                        );
                    } else {
                        group.bench_with_input(id, &payload[..], |b, payload| {
                            let mut state = black_box($name::state(creds(42).next().unwrap()));
                            b.iter(move || {
                                let _ = black_box($name::send(&mut state, payload));
                            });
                        });
                    }
                }};
            }

            // bench!(null);
            bench!(aes_128_gcm);
            bench!(aes_256_gcm);

            for (name, algo) in inline {
                group.bench_with_input(
                    BenchmarkId::new(format!("{name}_inline"), &input_name),
                    &(&header[..], &payload[..]),
                    |b, (header, payload)| {
                        let key = black_box(inline::key(algo));
                        let mut payload = black_box(payload.to_vec());
                        let mut packet_number = 0u32;
                        b.iter(move || {
                            let _ = black_box(inline::send(
                                &key,
                                &mut packet_number,
                                header,
                                &mut payload,
                            ));
                        });
                    },
                );

                group.bench_with_input(
                    BenchmarkId::new(format!("{name}_inline_scatter"), &input_name),
                    &(&header[..], &payload[..]),
                    |b, (header, payload)| {
                        let key = black_box(inline::key(algo));
                        let mut out = black_box(payload.to_vec());
                        out.extend(&[0u8; 16]);
                        let mut packet_number = 0u32;
                        b.iter(move || {
                            let _ = black_box(inline::send_scatter(
                                &key,
                                &mut packet_number,
                                header,
                                payload,
                                &mut out,
                            ));
                        });
                    },
                );
            }
        }
    }
    */
}

/*
mod inline {
    use aws_lc_rs::aead::{Aad, Algorithm, LessSafeKey, Nonce, UnboundKey, NONCE_LEN};

    #[inline(never)]
    pub fn key(algo: &'static Algorithm) -> LessSafeKey {
        let max_key = [42u8; 32];
        let key = &max_key[..algo.key_len()];
        let key = UnboundKey::new(algo, key).unwrap();
        LessSafeKey::new(key)
    }

    #[inline(never)]
    pub fn send(key: &LessSafeKey, packet_number: &mut u32, header: &[u8], payload: &mut [u8]) {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[NONCE_LEN - 8..].copy_from_slice(&(*packet_number as u64).to_be_bytes());
        let nonce = Nonce::assume_unique_for_key(nonce);

        let aad = Aad::from(header);
        let mut tag = [0u8; 16];
        key.seal_in_place_scatter(nonce, aad, payload, &[][..], &mut tag)
            .unwrap();

        *packet_number += 1;
    }

    #[inline(never)]
    pub fn send_scatter(
        key: &LessSafeKey,
        packet_number: &mut u32,
        header: &[u8],
        payload: &[u8],
        out: &mut [u8],
    ) {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[NONCE_LEN - 8..].copy_from_slice(&(*packet_number as u64).to_be_bytes());
        let nonce = Nonce::assume_unique_for_key(nonce);

        let aad = Aad::from(header);
        key.seal_in_place_scatter(nonce, aad, &mut [][..], payload, out)
            .unwrap();

        *packet_number += 1;
    }
}
*/

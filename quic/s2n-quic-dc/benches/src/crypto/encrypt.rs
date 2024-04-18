// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, BenchmarkId, Criterion, Throughput};

pub fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto/encrypt");

    let headers = [0, 16];

    let payloads = [1, 100, 1000, 8900];

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

            for (name, algo) in inline {
                group.bench_with_input(
                    BenchmarkId::new(format!("{name}_reuse"), &input_name),
                    &(&header[..], &payload[..]),
                    |b, (header, payload)| {
                        let key = black_box(awslc::key(algo));
                        let mut payload = black_box(payload.to_vec());
                        let mut packet_number = 0u32;
                        b.iter(move || {
                            let _ = black_box(awslc::encrypt(
                                &key,
                                &mut packet_number,
                                header,
                                &mut payload,
                            ));
                        });
                    },
                );

                group.bench_with_input(
                    BenchmarkId::new(format!("{name}_fresh"), &input_name),
                    &(&header[..], &payload[..]),
                    |b, (header, payload)| {
                        let mut payload = black_box(payload.to_vec());
                        let mut packet_number = 0u32;
                        b.iter(move || {
                            let key = black_box(awslc::key(algo));
                            let _ = black_box(awslc::encrypt(
                                &key,
                                &mut packet_number,
                                header,
                                &mut payload,
                            ));
                        });
                    },
                );
            }
        }
    }
}

mod awslc {
    use aws_lc_rs::aead::{Aad, Algorithm, LessSafeKey, Nonce, UnboundKey, NONCE_LEN};

    #[inline(never)]
    pub fn key(algo: &'static Algorithm) -> LessSafeKey {
        let max_key = [42u8; 32];
        let key = &max_key[..algo.key_len()];
        let key = UnboundKey::new(algo, key).unwrap();
        LessSafeKey::new(key)
    }

    #[inline(never)]
    pub fn encrypt(key: &LessSafeKey, packet_number: &mut u32, header: &[u8], payload: &mut [u8]) {
        let mut nonce = [0u8; NONCE_LEN];
        nonce[NONCE_LEN - 8..].copy_from_slice(&(*packet_number as u64).to_be_bytes());
        let nonce = Nonce::assume_unique_for_key(nonce);

        let aad = Aad::from(header);
        let mut tag = [0u8; 16];
        key.seal_in_place_scatter(nonce, aad, payload, &[][..], &mut tag)
            .unwrap();

        *packet_number += 1;
    }
}

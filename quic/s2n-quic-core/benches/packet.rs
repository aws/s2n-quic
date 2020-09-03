use criterion::{black_box, criterion_group, criterion_main, Benchmark, Criterion, Throughput};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::packet::ProtectedPacket;

fn decoding(c: &mut Criterion) {
    macro_rules! benchmark {
        ($name:expr) => {{
            let test = include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/packet/test_samples/",
                $name,
                ".bin"
            ));
            c.bench(
                $name,
                Benchmark::new($name, move |b| {
                    b.iter(move || {
                        // The decoder doesn't actually mutate the slice.
                        // Instead of testing the performance of memcpy we'll copy the mut slice
                        let mut data = black_box(unsafe {
                            std::slice::from_raw_parts_mut(test.as_ptr() as *mut u8, test.len())
                        });

                        let buffer = DecoderBufferMut::new(&mut data);
                        let _ = black_box(ProtectedPacket::decode(buffer, &20).unwrap());
                    })
                })
                .throughput(Throughput::Bytes(test.len() as u64)),
            );
        }};
    }

    benchmark!("short");
    benchmark!("initial");
    benchmark!("zero_rtt");
    benchmark!("handshake");
    benchmark!("retry");
    benchmark!("version_negotiation");
}

criterion_group!(benches, decoding);
criterion_main!(benches);

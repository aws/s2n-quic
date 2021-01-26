use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use s2n_codec::{DecoderBufferMut, Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::frame::FrameMut;

fn decode(c: &mut Criterion) {
    macro_rules! benchmark {
        ($name:expr) => {{
            fn get_test() -> &'static mut [u8] {
                let test = include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/frame/test_samples/",
                    $name,
                    ".bin"
                ));

                // The decoder doesn't actually mutate the slice.
                // Instead of testing the performance of memcpy we'll copy the mut slice
                black_box(unsafe {
                    std::slice::from_raw_parts_mut(test.as_ptr() as *mut u8, test.len())
                })
            }

            let mut group = c.benchmark_group($name);

            group.throughput(Throughput::Bytes(get_test().len() as u64));

            group.bench_function(concat!("decode ", $name), move |b| {
                b.iter(move || {
                    let mut test = get_test();
                    let buffer = DecoderBufferMut::new(&mut test);
                    let _ = black_box(buffer.decode::<FrameMut>().unwrap());
                });
            });

            group.bench_function(concat!("encode ", $name), move |b| {
                let mut test = get_test();
                let buffer = DecoderBufferMut::new(&mut test);
                let (frame, _remaining) = buffer.decode::<FrameMut>().unwrap();
                let mut out_data = vec![0; frame.encoding_size()];

                b.iter(move || EncoderBuffer::new(&mut out_data).encode(&frame));
            });

            group.finish();
        }};
    }

    benchmark!("ack");
    benchmark!("connection_close");
    benchmark!("crypto");
    benchmark!("data_blocked");
    benchmark!("handshake_done");
    benchmark!("max_data");
    benchmark!("max_stream_data");
    benchmark!("max_streams");
    benchmark!("new_connection_id");
    benchmark!("new_token");
    benchmark!("padding");
    benchmark!("path_challenge");
    benchmark!("path_response");
    benchmark!("ping");
    benchmark!("reset_stream");
    benchmark!("retire_connection_id");
    benchmark!("stop_sending");
    benchmark!("stream");
    benchmark!("stream_data_blocked");
    benchmark!("streams_blocked");
}

criterion_group!(benches, decode);
criterion_main!(benches);

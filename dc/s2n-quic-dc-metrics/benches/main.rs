use criterion::BenchmarkId;
use criterion::{criterion_group, criterion_main, Criterion};
use s2n_quic_dc_metrics::Registry;
use std::sync::atomic::{AtomicUsize, Ordering};

fn scale_counters(c: &mut Criterion) {
    let registry = Registry::new();
    let counter = registry.register_counter(String::from("counter"), None);

    check_scaling(&mut *c, "counter", || counter.increment(1));

    let hist = registry.register_summary(
        String::from("summary"),
        None,
        s2n_quic_dc_metrics::Unit::Count,
    );

    check_scaling(&mut *c, "summary", || hist.record_value(100));
}

// A perfectly scaling implementation will see time/iteration stay *constant* as the number of
// threads goes up. We do more work (each thread runs all iterations) but if we scale perfectly,
// that shouldn't cost us any more time.
fn check_scaling(c: &mut Criterion, name: &str, inner: impl Fn() + Send + Sync) {
    let inner = &inner;
    let mut group = c.benchmark_group(name);
    for threads in 1..std::thread::available_parallelism().unwrap().get() {
        group.bench_with_input(
            BenchmarkId::new("threads", threads),
            &(threads as u64),
            |b, threads| {
                b.iter_custom(|iterations| {
                    let start = std::time::Instant::now();
                    let finished = AtomicUsize::new(0);
                    let finished = &finished;
                    std::thread::scope(|s| {
                        for _ in 0..*threads {
                            s.spawn(move || {
                                for _ in 0..iterations {
                                    inner();
                                }
                                // Keep looping to keep contention constant.
                                finished.fetch_add(1, Ordering::Relaxed);
                                while finished.load(Ordering::Relaxed) != *threads as usize {
                                    // Don't have an atomic load every iteration.
                                    for _ in 0..100 {
                                        inner();
                                    }
                                }
                            });
                        }
                    });
                    start.elapsed()
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, scale_counters);
criterion_main!(benches);

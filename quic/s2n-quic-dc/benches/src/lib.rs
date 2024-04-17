use criterion::Criterion;

pub mod crypto;
pub mod datagram;

pub fn benchmarks(c: &mut Criterion) {
    crypto::benchmarks(c);
    datagram::benchmarks(c);
}

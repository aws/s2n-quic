use core::sync::atomic::{AtomicU64, Ordering};
use s2n_quic_core::varint::VarInt;

#[derive(Clone, Copy, Debug)]
pub struct ExhaustionError;

#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    #[inline]
    pub fn reset(&self) {
        self.0.store(0, Ordering::Relaxed)
    }

    #[inline]
    pub fn next(&self) -> Result<VarInt, ExhaustionError> {
        // https://marabos.nl/atomics/memory-ordering.html#relaxed
        // > While atomic operations using relaxed memory ordering do not
        // > provide any happens-before relationship, they do guarantee a total
        // > modification order of each individual atomic variable. This means
        // > that all modifications of the same atomic variable happen in an
        // > order that is the same from the perspective of every single thread.
        let pn = self.0.fetch_add(1, Ordering::Relaxed);
        VarInt::new(pn).map_err(|_| ExhaustionError)
    }
}

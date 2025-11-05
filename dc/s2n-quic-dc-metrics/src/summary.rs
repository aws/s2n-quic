use crate::{
    rseq::{Absorb, Channels},
    Unit,
};
use std::{fmt::Write as _, sync::Arc, time::Duration};

const FLOAT_INT_MULTIPLIER: f64 = 1000.0;

/// use to convert a float to an int
/// preserving digits in the decimal by
/// multiplying by FLOAT_INT_MULTIPLIER.
/// Convert back to float at logging time.
/// This is necessary due to a metrics storage
/// entrenched in only storing ints.
pub fn logging_util_float_to_integer(val: f64) -> u64 {
    (val * FLOAT_INT_MULTIPLIER) as u64
}

/// use to convert an int returned by
/// logging_util_float_to_integer() back to a float
/// preserving digits in the decimal. Necessary due to
/// metrics storage only storing ints.
fn logging_util_integer_to_float(val: u64) -> f64 {
    val as f64 / FLOAT_INT_MULTIPLIER
}

/// A `Summary` aggregates summary statistics. It is cheaper/smaller to add to compared to
/// `Collection` for cases where storing and reporting all individual data values may be too
/// expensive.
#[derive(Clone)]
pub struct Summary {
    channels: Arc<Channels<SharedSummary>>,
    idx: u32,
    display_unit: Unit,
}

const BUCKETS: usize = CONFIG.total_buckets();

pub(crate) struct SharedSummary {
    value: Box<[u64; BUCKETS]>,
}

impl Default for SharedSummary {
    fn default() -> Self {
        Self {
            // SAFETY: Slice to array conversion doesn't change the layout of the allocation.
            //
            // FIXME: Replace with https://doc.rust-lang.org/nightly/std/boxed/struct.Box.html#method.into_array
            // once it's stabilized.
            value: unsafe {
                Box::from_raw(
                    Box::into_raw(vec![0u64; BUCKETS].into_boxed_slice()) as *mut [u64; BUCKETS]
                )
            },
        }
    }
}

mod bucket;

// Ensure the maximum bucket fits into the space we've reserved for it.
const _: () = assert!(u16::MAX as u64 >= BUCKETS as u64);

impl Absorb for SharedSummary {
    fn handle(slots: &mut [Self], events: &mut [u64]) {
        let (chunks, tail) = events.as_chunks::<8>();
        for chunk in chunks {
            for event in chunk {
                let idx = (*event >> 16) as usize;
                slots[idx].value[*event as u16 as usize] += 1;
            }
        }

        for event in tail {
            let idx = (*event >> 16) as usize;
            slots[idx].value[*event as u16 as usize] += 1;
        }
    }
}

pub struct SummaryInner {
    display_unit: Unit,
    histogram: histogram::AtomicHistogram,
}

impl Summary {
    pub(crate) fn new(channels: Arc<Channels<SharedSummary>>, display_unit: Unit) -> Summary {
        let idx = channels.allocate();
        Summary {
            idx,
            display_unit,
            channels,
        }
    }

    pub fn record_value(&self, value: u64) {
        let Some(bucket) = CONFIG.value_to_index(value) else {
            return;
        };
        self.channels
            .send_event(((self.idx as u64) << 16) | bucket as u64);
    }

    pub fn record_duration(&self, duration: Duration) {
        assert!(matches!(
            self.display_unit,
            Unit::Microsecond | Unit::Second
        ));
        self.record_value(duration.as_nanos() as u64);
    }

    pub fn take_current(&self) -> Option<String> {
        Some(self.channels.get_mut(self.idx, |hist| {
            let res = format(&hist.value, self.display_unit);
            hist.value.as_mut_slice().fill(0);
            res
        }))
    }
}

fn format(hist: &[u64; BUCKETS], display_unit: Unit) -> String {
    let mut f = String::new();
    // Shouldn't be capable of overflowing -- u64 counter generally cannot overflow.
    let total_count = hist.iter().sum::<u64>();
    if total_count == 0 {
        f.push('0');
    } else {
        let quantiles = [
            0.0f64, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.99, 0.999, 1.0,
        ]
        .map(|q| (q * total_count as f64).ceil() as u64);
        let mut quantile_idx = 0;

        // Prefix sum up to the current bucket.
        let mut partial_count = 0;
        // Prefix sum excluding already reported counts (i.e., those we've written to `f`).
        let mut since_last_write = 0;
        let mut first = true;
        for (idx, bucket) in hist.iter().enumerate() {
            partial_count += *bucket;
            since_last_write += *bucket;

            // If this bucket hits the next quantile, we write out the current count.
            //
            // Can't panic due to the break below on partial_count == total_count.
            if partial_count >= quantiles[quantile_idx] {
                quantile_idx += 1;

                if since_last_write != 0 {
                    if !first {
                        f.push('+');
                    }
                    first = false;

                    // Use the midpoint of the bucket. We don't know where the actual value was and
                    // this gives a balance between overestimating and under estimating.
                    //
                    // Note that this is skewing our data up -- we're reporting the full count since
                    // the last reported quantile in *this* bucket.
                    let new_value = CONFIG
                        .index_to_lower_bound(idx)
                        .midpoint(CONFIG.index_to_upper_bound(idx));
                    let count = since_last_write;
                    since_last_write = 0;

                    let formatted_value = match display_unit {
                        Unit::Count | Unit::Byte | Unit::Percent => new_value,
                        Unit::Microsecond => Duration::from_nanos(new_value).as_micros() as u64,
                        Unit::Second => Duration::from_nanos(new_value).as_secs(),
                    };

                    match display_unit {
                        Unit::Percent => {
                            let formatted_value = logging_util_integer_to_float(formatted_value);
                            write!(f, "{formatted_value:.3}*{count}").unwrap();
                        }
                        _ => write!(f, "{formatted_value}*{count}").unwrap(),
                    }
                }
            }

            if partial_count == total_count {
                break;
            }
        }
    }

    write!(f, "{}", display_unit.pmet_str()).unwrap();

    f
}

const CONFIG: bucket::Config = bucket::Config::new(7, 64);

impl SummaryInner {
    pub fn new(display_unit: Unit) -> SummaryInner {
        SummaryInner {
            histogram: histogram::AtomicHistogram::new(
                CONFIG.grouping_power(),
                CONFIG.max_value_power(),
            )
            .unwrap(),
            display_unit,
        }
    }

    pub fn record_duration(&self, duration: Duration) {
        assert!(matches!(
            self.display_unit,
            Unit::Microsecond | Unit::Second
        ));
        self.record_value(duration.as_nanos() as u64);
    }

    pub fn record_value(&self, value: u64) {
        // This shouldn't fail because we set n=64 above in CONFIG.
        // Verified in a test case.
        self.histogram.increment(value).unwrap();
    }

    /// If reset is true, then this will reset the underlying histogram.
    pub fn format(&self, reset: bool) -> String {
        let mut f = String::new();
        let hist = if reset {
            self.histogram.drain()
        } else {
            self.histogram.load()
        };

        // Shouldn't be capable of overflowing -- u64 counter generally cannot overflow.
        let total_count = hist.as_slice().iter().sum::<u64>();
        if total_count == 0 {
            f.push('0');
        } else {
            let quantiles = [
                0.0f64, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.99, 0.999, 1.0,
            ]
            .map(|q| (q * total_count as f64).ceil() as u64);
            let mut quantile_idx = 0;

            // Prefix sum up to the current bucket.
            let mut partial_count = 0;
            // Prefix sum excluding already reported counts (i.e., those we've written to `f`).
            let mut since_last_write = 0;
            let mut first = true;
            for bucket in hist.iter() {
                partial_count += bucket.count();
                since_last_write += bucket.count();

                // If this bucket hits the next quantile, we write out the current count.
                //
                // Can't panic due to the break below on partial_count == total_count.
                if partial_count >= quantiles[quantile_idx] {
                    quantile_idx += 1;

                    if since_last_write != 0 {
                        if !first {
                            f.push('+');
                        }
                        first = false;

                        // Use the midpoint of the bucket. We don't know where the actual value was and
                        // this gives a balance between overestimating and under estimating.
                        //
                        // Note that this is skewing our data up -- we're reporting the full count since
                        // the last reported quantile in *this* bucket.
                        let new_value = bucket.start().midpoint(bucket.end());
                        let count = since_last_write;
                        since_last_write = 0;

                        let formatted_value = match self.display_unit {
                            Unit::Count | Unit::Byte | Unit::Percent => new_value,
                            Unit::Microsecond => Duration::from_nanos(new_value).as_micros() as u64,
                            Unit::Second => Duration::from_nanos(new_value).as_secs(),
                        };

                        match self.display_unit {
                            Unit::Percent => {
                                let formatted_value =
                                    logging_util_integer_to_float(formatted_value);
                                write!(f, "{formatted_value:.3}*{count}").unwrap();
                            }
                            _ => write!(f, "{formatted_value}*{count}").unwrap(),
                        }
                    }
                }

                if partial_count == total_count {
                    break;
                }
            }
        }

        write!(f, "{}", self.display_unit.pmet_str()).unwrap();

        f
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn count_correct() {
        let registry = crate::Registry::new();
        let summary = registry.register_summary(String::from("a"), None, Unit::Count);
        assert_eq!(registry.take_current_metrics_line(), "a=0");

        summary.record_value(0);
        summary.record_value(10);
        summary.record_value(20);
        summary.record_value(30);
        assert_eq!(registry.take_current_metrics_line(), "a=0*1+10*1+20*1+30*1");
        assert_eq!(registry.take_current_metrics_line(), "a=0");
    }

    #[test]
    fn visits_all_buckets() {
        let registry = crate::Registry::new();
        let summary = registry.register_summary(String::from("a"), None, Unit::Count);

        for bucket in 0..CONFIG.total_buckets() {
            let start = CONFIG.index_to_lower_bound(bucket);
            // Record a value from every bucket.
            summary.record_value(start);
        }

        assert_eq!(
            registry.take_current_metrics_line(),
            "a=0*1+3687*742+209407*742+11763711*743+643825663*742+34292629503*742+1979979923455*743+112425063940095*742+6315594789945343*743+345651271400685567*742+2531022990582218751*371+13078453317883920383*297+17906312118425092095*67+18410715276690587647*7"
        );
    }

    #[test]
    fn maximum() {
        let registry = crate::Registry::new();
        let summary = registry.register_summary(String::from("a"), None, Unit::Count);
        summary.record_value(u64::MAX);
        assert_eq!(
            registry.take_current_metrics_line(),
            "a=18410715276690587647*1"
        );
    }

    #[test]
    fn config() {
        assert_eq!(CONFIG.total_buckets(), 7424);
    }
}

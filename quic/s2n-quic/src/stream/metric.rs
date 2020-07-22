macro_rules! impl_metric_api {
    () => {
        /// Returns the current value of the given metric
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn metric<Metric: $crate::stream::Metric>(&self) -> $crate::stream::Result<Metric> {
            todo!()
        }
    };
}

use core::time::Duration;

/// Trait for a metric than can be queried on a stream
pub trait Metric {
    // TODO
}

/// The number of bytes sent on the stream
pub struct BytesSent(pub usize);

/// The number of bytes received on the stream
pub struct BytesReceived(pub usize);

/// The duration that the stream has been open
pub struct DurationOpen(pub Duration);

impl Metric for BytesSent {}
impl Metric for BytesReceived {}
impl Metric for DurationOpen {}

use core::time::Duration;

/// Trait for a metric than can be queried on a connection
pub trait Metric {
    // TODO
}

/// The total number of streams for the connection
pub struct StreamTotal(pub usize);

/// The number of open streams for the connection
pub struct StreamsOpen(pub usize);

/// The number of clos streams for the connection
pub struct StreamsClosed(pub usize);

/// The number of bytes sent on the connection
pub struct BytesSent(pub usize);

/// The number of bytes received on the connection
pub struct BytesReceived(pub usize);

/// The duration that the stream has been open
pub struct DurationOpen(pub Duration);

impl Metric for StreamTotal {}
impl Metric for StreamsOpen {}
impl Metric for StreamsClosed {}
impl Metric for BytesSent {}
impl Metric for BytesReceived {}
impl Metric for DurationOpen {}

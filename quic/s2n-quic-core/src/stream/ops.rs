//! Bulk operations performed on streams
//!
//! By representing stream operations as structs, callers can request multiple tasks to be
//! performed in a single call, which reduces context switching.
//!
//! Consider the following scenario where we send 3 chunks of data and finish the stream:
//!
//! ```rust,ignore
//! stream.send(a).await?;
//! stream.send(b).await?;
//! stream.send(c).await?;
//! stream.finish().await?;
//! ```
//!
//! This will result in at least 4 context switches (and potentially even more if the stream
//! is currently blocked on sending).
//!
//! Using the bulk operation API greatly reduces this amount:
//!
//! ```rust,ignore
//! stream
//!     .request()
//!     .send(&mut [a, b, c])
//!     .finish()
//!     .await?;
//! ```

use crate::application::ApplicationErrorCode;
use core::task::Poll;

/// A request made on a stream
#[derive(Default, Debug)]
pub struct Request<'a, Chunk> {
    /// The `tx` options of the request
    pub tx: Option<tx::Request<'a, Chunk>>,

    /// The `rx` options of the request
    pub rx: Option<rx::Request<'a, Chunk>>,
}

impl<'a, Chunk> Request<'a, Chunk> {
    /// Requests a slice of chunks to be sent on the tx stream
    pub fn send(&mut self, chunks: &'a mut [Chunk]) -> &mut Self {
        self.tx_mut().chunks = Some(chunks);
        self
    }

    /// Resets the tx stream with an error code
    pub fn reset(&mut self, error: ApplicationErrorCode) -> &mut Self {
        self.tx_mut().reset = Some(error);
        self
    }

    /// Flushes any pending tx data to be ACKed before unblocking
    pub fn flush(&mut self) -> &mut Self {
        self.tx_mut().flush = true;
        self
    }

    /// Marks the tx stream as finished (e.g. no more data will be sent)
    pub fn finish(&mut self) -> &mut Self {
        self.tx_mut().finish = true;
        self
    }

    /// Requests data on the rx stream to be received into the provided slice of chunks
    pub fn receive(&mut self, chunks: &'a mut [Chunk]) -> &mut Self {
        self.rx_mut().chunks = Some(chunks);
        self
    }

    /// Requests the peer to stop sending data on the rx stream
    pub fn stop_sending(&mut self, error: ApplicationErrorCode) -> &mut Self {
        self.rx_mut().stop_sending = Some(error);
        self
    }

    /// Sets the watermarks for the rx stream
    pub fn with_watermark(&mut self, low: usize, high: usize) -> &mut Self {
        let rx = self.rx_mut();
        rx.low_watermark = low.min(high);
        rx.high_watermark = high.max(low);
        self
    }

    /// Sets the low watermark for the rx stream
    ///
    /// If the watermark is set to `0`, the caller will be notified as soon as there is data
    /// available on the stream.
    ///
    /// If the watermark is greater than `0`, the caller will be notified as soon as there is at
    /// least `low` bytes available to be read. Note that the stream may be woken earlier.
    pub fn with_low_watermark(&mut self, low: usize) -> &mut Self {
        let rx = self.rx_mut();
        rx.low_watermark = low;
        // raise the high watermark to be at least the lower
        rx.high_watermark = rx.high_watermark.max(low);
        self
    }

    /// Sets the high watermark for the rx stream
    ///
    /// The stream ensures that all the received data will not exceed the watermark amount. This
    /// can be useful for receiving at most `n` bytes.
    pub fn with_high_watermark(&mut self, high: usize) -> &mut Self {
        let rx = self.rx_mut();
        rx.high_watermark = high;
        // lower the low watermark to be less than the higher
        rx.low_watermark = rx.low_watermark.min(high);
        self
    }

    /// Lazily creates and returns the `tx` request
    fn tx_mut(&mut self) -> &mut tx::Request<'a, Chunk> {
        if self.tx.is_none() {
            self.tx = Some(Default::default());
        }
        self.tx.as_mut().expect("tx should always be initialized")
    }

    /// Lazily creates and returns the `rx` request
    fn rx_mut(&mut self) -> &mut rx::Request<'a, Chunk> {
        if self.rx.is_none() {
            self.rx = Some(Default::default());
        }
        self.rx.as_mut().expect("rx should always be initialized")
    }
}

/// A response received after executing a request
#[derive(Debug, Default, PartialEq)]
pub struct Response {
    /// The `tx` information of the response
    pub tx: Option<tx::Response>,

    /// The `rx` information of the response
    pub rx: Option<rx::Response>,
}

impl Response {
    /// Returns `true` if either the `rx` or `tx` requests will wake the provided waker at a later
    /// point in time.
    pub fn is_pending(&self) -> bool {
        self.tx.iter().any(|tx| tx.is_pending()) || self.rx.iter().any(|rx| rx.is_pending())
    }

    /// Returns the `tx` response
    pub fn tx(&self) -> Option<&tx::Response> {
        self.tx.as_ref()
    }

    /// Returns the `rx` response
    pub fn rx(&self) -> Option<&rx::Response> {
        self.rx.as_ref()
    }
}

/// Request and response related to transmitting on a stream
pub mod tx {
    use super::*;

    /// A request on a `tx` stream
    #[derive(Debug)]
    pub struct Request<'a, Chunk> {
        /// Optionally transmit chunks onto the stream
        ///
        /// The chunks will be replaced with empty buffers as they are stored in the transmission
        /// buffer. The response will indicate how many chunks and bytes were consumed from
        /// this slice.
        pub chunks: Option<&'a mut [Chunk]>,

        /// Optionally reset the stream with an error
        pub reset: Option<ApplicationErrorCode>,

        /// Waits for an ACK on resets and finishes
        pub flush: bool,

        /// Marks the tx stream as finished (e.g. no more data will be sent)
        pub finish: bool,
    }

    impl<'a, Chunk> Default for Request<'a, Chunk> {
        fn default() -> Self {
            Self {
                chunks: None,
                reset: None,
                flush: false,
                finish: false,
            }
        }
    }

    /// The result of a tx request
    #[derive(Debug, Default, PartialEq)]
    pub struct Response {
        /// Information about the bytes that were sent
        pub bytes: Bytes,

        /// Information about the chunks that were sent
        pub chunks: Chunks,

        /// Indicates if the operation resulted in storing the provided waker to notify when the
        /// request may be polled again.
        pub will_wake: bool,

        /// Indicates if the operation resulted ending the stream.
        pub is_finished: bool,
    }

    impl Response {
        /// Returns true if provided waker will be woken
        pub fn is_pending(&self) -> bool {
            self.will_wake
        }

        /// Returns the `tx` response
        pub fn tx(&self) -> Option<&Self> {
            Some(self)
        }
    }
}

/// Request and response related to receiving on a stream
pub mod rx {
    use super::*;

    /// A request on a `tx` stream
    #[derive(Debug)]
    pub struct Request<'a, Chunk> {
        /// Optionally receive chunks from the stream
        ///
        /// At least one of the provided chunks should be empty, as it will be replaced by the
        /// received data from the stream. The response will indicate how many chunks and
        /// bytes were consumed from the stream into the provided slice.
        pub chunks: Option<&'a mut [Chunk]>,

        /// Sets the low watermark for the rx stream
        ///
        /// If the watermark is set to `0`, the caller will be notified as soon as there is data
        /// available on the stream.
        ///
        /// If the watermark is greater than `0`, the caller will be notified as soon as there is at
        /// least `low` bytes available to be read. Note that the stream may be woken earlier.
        pub low_watermark: usize,

        /// Sets the high watermark for the rx stream
        ///
        /// The stream ensures that all the received data will not exceed the watermark amount. This
        /// can be useful for receiving at most `n` bytes.
        pub high_watermark: usize,

        /// Optionally requests the peer to stop sending data with an error
        pub stop_sending: Option<ApplicationErrorCode>,
    }

    impl<'a, Chunk> Default for Request<'a, Chunk> {
        fn default() -> Self {
            Self {
                chunks: None,
                low_watermark: 0,
                high_watermark: core::usize::MAX,
                stop_sending: None,
            }
        }
    }

    /// The result of a pop operation
    #[derive(Debug, Default, PartialEq)]
    pub struct Response {
        /// Information about the bytes that were received
        pub bytes: Bytes,

        /// Information about the chunks that were received
        pub chunks: Chunks,

        /// Indicates if the operation resulted in storing the provided waker to notify when the
        /// request may be polled again.
        pub will_wake: bool,

        /// Indicates that the stream's available bytes and chunks are not going to increase beyond
        /// their reported size, as the stream has reached the end
        pub is_final: bool,

        /// Indicates that the request asked the peer to stop sending data.
        pub is_stopped: bool,
    }

    impl Response {
        /// Returns true if provided waker will be woken
        pub fn is_pending(&self) -> bool {
            self.will_wake
        }

        /// Returns the `rx` response
        pub fn rx(&self) -> Option<&Self> {
            Some(self)
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Bytes {
    /// The number of bytes that were consumed by the operation.
    ///
    /// In the case of `tx` operations, this is the number of bytes that were sent on the
    /// stream.
    ///
    /// In the case of `rx` operations, this is the number of bytes that were received from the
    /// stream.
    pub consumed: usize,

    /// The number of bytes that are available on the stream.
    ///
    /// In the case of `tx` operations, this is the number of additional bytes that can be sent
    /// in the stream. Note that this is not a hard limit on accepting a chunk of data.
    ///
    /// In the case of `rx` operations, this is the number of additional bytes that can be received
    /// from the stream.
    pub available: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct Chunks {
    /// The number of chunks that were consumed by the operation.
    ///
    /// In the case of `tx` operations, this is the number of chunks that were sent on the
    /// stream.
    ///
    /// In the case of `rx` operations, this is the number of chunks that were received from the
    /// stream.
    pub consumed: usize,

    /// The number of chunks that are available on the stream.
    ///
    /// In the case of `tx` operations, this is the number of additional chunks that can be sent
    /// in the stream. This value will be based on the assumption of 1 byte chunks and will
    /// contain the same value of `bytes.available`.
    ///
    /// In the case of `rx` operations, this is the number of additional chunks that can be received
    /// from the stream.
    pub available: usize,
}

macro_rules! conversions {
    ($name:path) => {
        impl $name {
            /// Converts the response into a `Poll<Self>`
            pub fn into_poll(self) -> Poll<Self> {
                if self.is_pending() {
                    Poll::Pending
                } else {
                    Poll::Ready(self)
                }
            }
        }

        impl Into<()> for $name {
            fn into(self) {}
        }

        impl<T, E> Into<Poll<Result<T, E>>> for $name
        where
            $name: Into<T>,
        {
            fn into(self) -> Poll<Result<T, E>> {
                if self.is_pending() {
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(self.into()))
                }
            }
        }
    };
}

conversions!(Response);
conversions!(tx::Response);
conversions!(rx::Response);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_builder_test() {
        let mut request = Request::default();
        let mut send_chunks = [1, 2];
        let mut receive_chunks = [3, 4];

        request
            .send(&mut send_chunks)
            .finish()
            .flush()
            .reset(ApplicationErrorCode::new(1).unwrap())
            .receive(&mut receive_chunks)
            .with_watermark(5, 10)
            .stop_sending(ApplicationErrorCode::new(2).unwrap());

        assert!(matches!(
            request,
            Request {
                tx: Some(tx::Request {
                    chunks: Some(&mut [1, 2]),
                    finish: true,
                    flush: true,
                    reset: Some(reset),
                }),
                rx: Some(rx::Request {
                    chunks: Some(&mut [3, 4]),
                    low_watermark: 5,
                    high_watermark: 10,
                    stop_sending: Some(stop_sending)
                })
            } if reset == ApplicationErrorCode::new(1).unwrap()
              && stop_sending == ApplicationErrorCode::new(2).unwrap()
        ));
    }

    #[test]
    fn response_pending_test() {
        for rx_pending in [false, true].iter().cloned() {
            for tx_pending in [false, true].iter().cloned() {
                let response = Response {
                    tx: Some(tx::Response {
                        will_wake: tx_pending,
                        ..Default::default()
                    }),
                    rx: Some(rx::Response {
                        will_wake: rx_pending,
                        ..Default::default()
                    }),
                };

                assert_eq!(response.is_pending(), rx_pending || tx_pending);

                if rx_pending || tx_pending {
                    assert_eq!(response.into_poll(), Poll::Pending);
                } else {
                    assert_eq!(
                        response.into_poll(),
                        Poll::Ready(Response {
                            tx: Some(tx::Response::default()),
                            rx: Some(rx::Response::default()),
                        })
                    );
                }
            }
        }
    }
}

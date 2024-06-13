// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

use crate::{application, stream};
use core::task::Poll;

/// A request made on a stream
#[derive(Default, Debug)]
pub struct Request<'a> {
    /// The `tx` options of the request
    pub tx: Option<tx::Request<'a>>,

    /// The `rx` options of the request
    pub rx: Option<rx::Request<'a>>,
}

impl<'a> Request<'a> {
    /// Requests a slice of chunks to be sent on the tx stream
    pub fn send(&mut self, chunks: &'a mut [bytes::Bytes]) -> &mut Self {
        self.tx_mut().chunks = Some(chunks);
        self
    }

    /// Resets the tx stream with an error code
    pub fn reset(&mut self, error: application::Error) -> &mut Self {
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
    pub fn receive(&mut self, chunks: &'a mut [bytes::Bytes]) -> &mut Self {
        self.rx_mut().chunks = Some(chunks);
        self
    }

    /// Requests the peer to stop sending data on the rx stream
    pub fn stop_sending(&mut self, error: application::Error) -> &mut Self {
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

    pub fn detach_tx(&mut self) -> &mut Self {
        let tx = self.tx_mut();
        tx.detached = true;
        self
    }

    pub fn detach_rx(&mut self) -> &mut Self {
        let rx = self.rx_mut();
        rx.detached = true;
        self
    }

    /// Lazily creates and returns the `tx` request
    fn tx_mut(&mut self) -> &mut tx::Request<'a> {
        if self.tx.is_none() {
            self.tx = Some(Default::default());
        }
        self.tx.as_mut().expect("tx should always be initialized")
    }

    /// Lazily creates and returns the `rx` request
    fn rx_mut(&mut self) -> &mut rx::Request<'a> {
        if self.rx.is_none() {
            self.rx = Some(Default::default());
        }
        self.rx.as_mut().expect("rx should always be initialized")
    }
}

/// A response received after executing a request
#[derive(Debug, Default, PartialEq, Eq)]
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
    #[derive(Default, Debug)]
    pub struct Request<'a> {
        /// Optionally transmit chunks onto the stream
        ///
        /// The chunks will be replaced with empty buffers as they are stored in the transmission
        /// buffer. The response will indicate how many chunks and bytes were consumed from
        /// this slice.
        pub chunks: Option<&'a mut [bytes::Bytes]>,

        /// Optionally reset the stream with an error
        pub reset: Option<application::Error>,

        /// Waits for an ACK on resets and finishes
        pub flush: bool,

        /// Marks the tx stream as finished (e.g. no more data will be sent)
        pub finish: bool,

        /// Marks the tx stream as detached, which makes the stream make progress, regardless of
        /// application observations.
        pub detached: bool,
    }

    /// The result of a tx request
    #[derive(Debug, PartialEq, Eq)]
    pub struct Response {
        /// Information about the bytes that were sent
        pub bytes: Bytes,

        /// Information about the chunks that were sent
        pub chunks: Chunks,

        /// Indicates if the operation resulted in storing the provided waker to notify when the
        /// request may be polled again.
        pub will_wake: bool,

        /// The current status of the stream
        pub status: Status,
    }

    impl Default for Response {
        fn default() -> Self {
            Self {
                bytes: Bytes::default(),
                chunks: Chunks::default(),
                will_wake: false,
                status: Status::Open,
            }
        }
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

    /// A request on a `rx` stream
    #[derive(Debug)]
    pub struct Request<'a> {
        /// Optionally receive chunks from the stream
        ///
        /// At least one of the provided chunks should be empty, as it will be replaced by the
        /// received data from the stream. The response will indicate how many chunks and
        /// bytes were consumed from the stream into the provided slice.
        pub chunks: Option<&'a mut [bytes::Bytes]>,

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
        pub stop_sending: Option<application::Error>,

        /// Marks the rx stream as detached, which makes the stream make progress, regardless of
        /// application observations.
        pub detached: bool,
    }

    impl<'a> Default for Request<'a> {
        fn default() -> Self {
            Self {
                chunks: None,
                low_watermark: 0,
                high_watermark: usize::MAX,
                stop_sending: None,
                detached: false,
            }
        }
    }

    /// The result of a pop operation
    #[derive(Debug, PartialEq, Eq)]
    pub struct Response {
        /// Information about the bytes that were received
        pub bytes: Bytes,

        /// Information about the chunks that were received
        pub chunks: Chunks,

        /// Indicates if the operation resulted in storing the provided waker to notify when the
        /// request may be polled again.
        pub will_wake: bool,

        /// The current status of the stream
        pub status: Status,
    }

    impl Default for Response {
        fn default() -> Self {
            Self {
                bytes: Bytes::default(),
                chunks: Chunks::default(),
                will_wake: false,
                status: Status::Open,
            }
        }
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

#[derive(Debug, Default, PartialEq, Eq)]
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

#[derive(Debug, Default, PartialEq, Eq)]
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The stream is open and writable
    Open,

    /// The stream is finishing but still has data to be flushed
    Finishing,

    /// The stream is finished and completely flushed
    Finished,

    /// The stream has been reset locally but has not been acknowledged by the peer
    Resetting,

    /// The stream was reset either by the peer or locally
    Reset(stream::StreamError),
}

macro_rules! impl_status {
    (| $self:ident | $value:expr) => {
        /// Returns `true` if the status is `Open`
        pub fn is_open(&self) -> bool {
            matches!(self.status(), Status::Open)
        }

        /// Returns `true` if the status is `Finishing`
        pub fn is_finishing(&self) -> bool {
            matches!(self.status(), Status::Finishing)
        }

        /// Returns `true` if the status is `Finished`
        pub fn is_finished(&self) -> bool {
            matches!(self.status(), Status::Finished)
        }

        /// Returns `true` if the status is `Resetting`
        pub fn is_resetting(&self) -> bool {
            matches!(self.status(), Status::Resetting)
        }

        /// Returns `true` if the status is `Reset`
        pub fn is_reset(&self) -> bool {
            matches!(self.status(), Status::Reset(_))
        }

        /// Returns `true` if the status is `Finishing` or `Resetting`
        pub fn is_closing(&self) -> bool {
            self.is_finishing() || self.is_resetting()
        }

        /// Returns `true` if the status is `Finished` or `Reset`
        pub fn is_closed(&self) -> bool {
            self.is_finished() || self.is_reset()
        }

        const fn status(&$self) -> Status {
            $value
        }
    };
}

impl Status {
    impl_status!(|self| *self);
}

impl rx::Response {
    impl_status!(|self| self.status);
}

impl tx::Response {
    impl_status!(|self| self.status);
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

        impl From<$name> for () {
            fn from(_: $name) {}
        }

        impl<T, E> From<$name> for Poll<Result<T, E>>
        where
            $name: Into<T>,
        {
            fn from(v: $name) -> Poll<Result<T, E>> {
                if v.is_pending() {
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(v.into()))
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
        let mut send_chunks = [bytes::Bytes::from_static(&[1])];
        let mut receive_chunks = [
            bytes::Bytes::from_static(&[2]),
            bytes::Bytes::from_static(&[3]),
        ];

        request
            .send(&mut send_chunks)
            .finish()
            .flush()
            .reset(application::Error::new(1).unwrap())
            .receive(&mut receive_chunks)
            .with_watermark(5, 10)
            .stop_sending(application::Error::new(2).unwrap());

        assert!(matches!(
            request,
            Request {
                tx: Some(tx::Request {
                    chunks: Some(tx_chunks),
                    finish: true,
                    flush: true,
                    reset: Some(reset),
                    detached: false,
                }),
                rx: Some(rx::Request {
                    chunks: Some(rx_chunks),
                    low_watermark: 5,
                    high_watermark: 10,
                    stop_sending: Some(stop_sending),
                    detached: false,
                })
            } if reset == application::Error::new(1).unwrap()
              && stop_sending == application::Error::new(2).unwrap()
              && tx_chunks.len() == 1
              && rx_chunks.len() == 2
        ));
    }

    #[test]
    fn response_pending_test() {
        for rx_pending in [false, true] {
            for tx_pending in [false, true] {
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

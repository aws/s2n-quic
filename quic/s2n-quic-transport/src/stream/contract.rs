// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;
use core::task::Context;
use s2n_quic_core::{
    application,
    stream::{ops, StreamError},
};

/// Request snapshot
pub struct Request {
    tx: Option<tx::Request>,
    rx: Option<rx::Request>,
}

impl<'a> From<&'a ops::Request<'a>> for Request {
    fn from(req: &'a ops::Request<'a>) -> Self {
        Self {
            tx: req.tx.as_ref().map(|req| req.into()),
            rx: req.rx.as_ref().map(|req| req.into()),
        }
    }
}

impl Request {
    pub fn validate_response(
        &self,
        request: &ops::Request,
        response: Result<&ops::Response, &StreamError>,
        context: Option<&Context>,
    ) {
        macro_rules! validate_response {
            ($ty:ident) => {
                if let Some(req) = self.$ty.as_ref() {
                    let res = response.map(|res| {
                        res.$ty
                            .as_ref()
                            .expect(concat!(stringify!($ty), " request should yield a response"))
                    });

                    let original = request
                        .$ty
                        .as_ref()
                        .expect("original request should always match snapshot");

                    req.validate_response(original, res, context);
                } else if let Ok(response) = response {
                    assert_eq!(response.$ty, None)
                }
            };
        }

        validate_response!(tx);
        validate_response!(rx);
    }
}

pub mod tx {
    use super::*;

    pub struct Request {
        chunks: Option<Vec<Bytes>>,
        reset: Option<application::Error>,
        flush: bool,
        finish: bool,
    }

    impl<'a> From<&'a ops::tx::Request<'a>> for Request {
        fn from(tx: &'a ops::tx::Request) -> Self {
            Self {
                chunks: tx.chunks.as_ref().map(|chunks| chunks.to_vec()),
                reset: tx.reset,
                flush: tx.flush,
                finish: tx.finish,
            }
        }
    }

    impl Request {
        #[allow(clippy::cognitive_complexity)]
        pub fn validate_response(
            &self,
            request: &ops::tx::Request,
            response: Result<&ops::tx::Response, &StreamError>,
            context: Option<&Context>,
        ) {
            // general response consistency checks
            if let Ok(response) = response {
                if response.will_wake {
                    assert!(
                        context.is_some(),
                        "will_wake should only be set when a context is provided"
                    );
                }

                if response.is_finished() {
                    assert_eq!(
                        response.bytes.available, 0,
                        "a finished stream should never report available bytes"
                    );
                    assert_eq!(
                        response.chunks.available, 0,
                        "a finished stream should never report available chunks"
                    );
                }
            }

            // resetting takes priority
            if self.reset.is_some() || response.is_ok_and(|res| res.is_reset()) {
                let response = response.expect("reset should never fail");

                assert_eq!(
                    response.bytes,
                    Default::default(),
                    "resetting should always return empty bytes"
                );
                assert_eq!(
                    response.chunks,
                    Default::default(),
                    "resetting should always return empty chunks"
                );

                if response.will_wake {
                    assert!(self.flush, "resetting should only wake on flush");
                    assert!(
                        response.is_resetting(),
                        "resetting with flush should report a Resetting status"
                    );
                } else if self.flush && context.is_some() {
                    assert!(
                        response.is_reset(),
                        "resetting should always set the finish flag"
                    );
                } else {
                    assert!(
                        response.is_resetting() || response.is_reset(),
                        "resetting with flush should report a Resetting status"
                    );
                }

                // none of the other checks apply so return early
                return;
            }

            // the request is interested in push availability
            if self
                .chunks
                .as_ref()
                .map_or(true, |chunks| chunks.is_empty())
                && !self.finish
                && !self.flush
                && context.is_some()
            {
                if let Ok(response) = response {
                    assert!(
                        !response.is_closing() && !response.is_closed(),
                        "availability queries should not finish"
                    );
                    assert_eq!(
                        response.bytes.consumed, 0,
                        "availability queries should never consume bytes"
                    );
                    assert_eq!(
                        response.chunks.consumed, 0,
                        "availability queries should never consume chunks"
                    );

                    if response.will_wake {
                        assert_eq!(
                            response.bytes,
                            Default::default(),
                            "response should only wake when no bytes available"
                        );
                        assert_eq!(
                            response.chunks,
                            Default::default(),
                            "response should only wake when no chunks available"
                        );
                    } else {
                        assert_ne!(
                            response.bytes.available, 0,
                            "response should wake when no bytes available"
                        );
                        assert_ne!(
                            response.chunks.available, 0,
                            "response should wake when no chunks available"
                        );
                    }
                }

                // none of the other checks apply so return early
                return;
            }

            if let Some(chunks) = self.chunks.as_ref() {
                let original = request.chunks.as_ref().unwrap();

                if let Ok(response) = response {
                    // make sure the consumed chunks line up with the byte lengths
                    let mut actual_bytes = 0;
                    for chunk in chunks.iter().take(response.chunks.consumed) {
                        actual_bytes += chunk.len();
                    }
                    assert_eq!(
                        response.bytes.consumed, actual_bytes,
                        "consumed bytes should be reported accurately"
                    );

                    // make sure all of the consumed chunks are empty in the original slice
                    for chunk in original.iter().take(response.chunks.consumed) {
                        assert!(chunk.is_empty(), "consumed chunks should always be empty");
                    }

                    assert_eq!(
                        chunks.len(),
                        original.len(),
                        "snapshot should always have the same number of chunks as the original request"
                    );

                    // make sure the remaining chunks haven't been modified
                    for (original, snapshot) in chunks
                        .iter()
                        .zip(original.iter())
                        .skip(response.chunks.consumed)
                    {
                        assert_eq!(
                            original, snapshot,
                            "non-consumed chunks should not be modified"
                        );
                    }

                    if response.will_wake {
                        let mut should_wake = false;
                        // empty chunks means we're querying status
                        should_wake |= chunks.is_empty();
                        // not all of the chunks were consumed
                        should_wake |= response.chunks.consumed < chunks.len();
                        // the request wanted all of the chunks to be flushed
                        should_wake |= self.flush;
                        assert!(
                            should_wake,
                            concat!(
                                "waker should only wake when not all of the provided chunks were consumed ",
                                "or when a flush was requested",
                            )
                        );
                    } else if context.is_some() {
                        assert_eq!(
                            response.chunks.consumed,
                            chunks.len(),
                            concat!(
                                "if a context was provided and will_wake was not set, all of the ",
                                "provided chunks should be consumed"
                            )
                        );
                    }
                }
            }

            if self.finish {
                if let Ok(response) = response {
                    // finishing only happens after we don't consume anything
                    if response.chunks.consumed == 0 {
                        if response.will_wake {
                            assert!(
                            response.is_finishing(),
                            "finished responses that will_wake should have a Finishing status; actual: {:?}",
                            response.status
                        );
                            assert!(
                                response.chunks.consumed < self.chunks_len() || self.flush,
                                concat!(
                                    "waker should only wake when not all of the provided chunks were consumed ",
                                    "or when a flush was requested",
                                ),
                            );
                        } else if self.flush && context.is_some() {
                            assert!(response.is_finished());
                        } else {
                            assert!(
                                response.is_finishing() || response.is_finished(),
                                "finishing should transition to finish status; actual: {:?}",
                                response.status
                            );
                        }
                    }
                }
            }

            if self.flush {
                if let Ok(response) = response {
                    if !response.will_wake && context.is_some() && response.is_open() {
                        assert_ne!(
                            response.bytes.available, 0,
                            "flushing should result in available bytes"
                        );
                        assert_ne!(
                            response.chunks.available, 0,
                            "flushing should result in available chunks"
                        );
                    }
                }
            }
        }

        fn chunks_len(&self) -> usize {
            self.chunks.as_ref().map(|chunks| chunks.len()).unwrap_or(0)
        }
    }
}

pub mod rx {
    use super::*;

    pub struct Request {
        chunks: Option<Vec<Bytes>>,
        low_watermark: usize,
        high_watermark: usize,
        stop_sending: Option<application::Error>,
    }

    impl<'a> From<&'a ops::rx::Request<'a>> for Request {
        fn from(rx: &'a ops::rx::Request) -> Self {
            Self {
                chunks: rx.chunks.as_ref().map(|chunks| chunks.to_vec()),
                high_watermark: rx.high_watermark,
                low_watermark: rx.low_watermark,
                stop_sending: rx.stop_sending,
            }
        }
    }

    impl Request {
        #[allow(clippy::cognitive_complexity)]
        pub fn validate_response(
            &self,
            request: &ops::rx::Request,
            response: Result<&ops::rx::Response, &StreamError>,
            context: Option<&Context>,
        ) {
            // general response consistency checks
            if let Ok(response) = response {
                if response.will_wake {
                    assert!(
                        context.is_some(),
                        "will_wake should only be set when a context was provided"
                    );
                }

                if (response.is_open() || response.is_finishing()) && response.will_wake {
                    assert_eq!(
                        response.bytes.consumed, 0,
                        "will_wake should only be set when nothing was consumed"
                    );
                    assert_eq!(
                        response.chunks.consumed, 0,
                        "will_wake should only be set when nothing was consumed"
                    );
                }

                if response.is_finishing() && context.is_some() && !response.will_wake {
                    assert_ne!(
                        response.bytes.available, 0,
                        "a finishing stream should always report available bytes",
                    );
                    assert_ne!(
                        response.chunks.available, 0,
                        "a finishing stream should always report available chunks",
                    );
                }

                if response.is_resetting() || response.is_closed() {
                    assert!(!response.will_wake, "a closed stream should never wake");
                    assert_eq!(
                        response.bytes.available, 0,
                        "a closed stream should never report available bytes"
                    );
                    assert_eq!(
                        response.chunks.available, 0,
                        "a closed stream should never report available chunks"
                    );
                }

                assert_eq!(
                    request.low_watermark,
                    self.low_watermark.saturating_sub(response.bytes.consumed),
                    "the low watermark should be lowered as data is consumed"
                );

                assert_eq!(
                    request.high_watermark,
                    self.high_watermark.saturating_sub(response.bytes.consumed),
                    "the high watermark should be lowered as data is consumed"
                );

                assert!(
                    self.high_watermark >= response.bytes.consumed,
                    "the number of bytes consumed should not exceed the high watermark"
                );
            }

            if self.stop_sending.is_some() {
                let response = response.expect("stop_sending should never fail");
                assert!(
                    response.is_reset() || response.is_finished(),
                    "stop_sending should reset or finish the stream"
                );

                assert_eq!(
                    response.bytes,
                    Default::default(),
                    "resetting should always return empty bytes"
                );
                assert_eq!(
                    response.chunks,
                    Default::default(),
                    "resetting should always return empty chunks"
                );

                // none of the other checks apply so return early
                return;
            }

            if let Some(chunks) = self.chunks.as_ref() {
                // if any of the provided chunks are non-empty, it should return an error
                let should_error = chunks.iter().any(|chunk| !chunk.is_empty());

                if should_error {
                    assert!(matches!(response, Err(StreamError::NonEmptyOutput { .. })));
                    return;
                }

                if let Ok(response) = response {
                    let mut iter = request.chunks.as_ref().unwrap().iter();

                    // add up all of the actual bytes returned
                    let mut actual_bytes = 0;
                    for _ in 0..response.chunks.consumed {
                        let chunk = iter.next().unwrap();
                        actual_bytes += chunk.len();
                    }

                    assert_eq!(
                        actual_bytes, response.bytes.consumed,
                        "reported consumed bytes should reflect the output chunks"
                    );

                    for chunk in iter {
                        assert!(
                            chunk.is_empty(),
                            "chunks beyond the reported consumed length should be empty"
                        );
                    }
                }
            }
        }
    }
}

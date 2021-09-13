// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{ReceiveStream, SendStream};

/// A trait that enables a stream to split into a [`ReceiveStream`] and [`SendStream`].
///
/// Note that if a stream is only allowed to send, then the receiving side will be [`None`].
/// The same is true for streams that are only allowed to receive: the sending side will be [`None`].
pub trait SplittableStream {
    /// Splits the stream into [`ReceiveStream`] and [`SendStream`] halves
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    fn split(self) -> (Option<ReceiveStream>, Option<SendStream>);
}

macro_rules! impl_splittable_stream_api {
    () => {
        /// Splits the stream into [`crate::stream::ReceiveStream`] and [`crate::stream::SendStream`] halves
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn split(
            self,
        ) -> (
            Option<$crate::stream::ReceiveStream>,
            Option<$crate::stream::SendStream>,
        ) {
            $crate::stream::SplittableStream::split(self)
        }
    };
}

macro_rules! impl_splittable_stream_trait {
    ($name:ident, | $self:ident | $convert:expr) => {
        impl $crate::stream::SplittableStream for $name {
            fn split(
                self,
            ) -> (
                Option<$crate::stream::ReceiveStream>,
                Option<$crate::stream::SendStream>,
            ) {
                let $self = self;
                $convert
            }
        }
    };
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_connection_api {
    (| $self:ident | $convert:expr) => {
        /// Returns the [`connection::Handle`](crate::connection::Handle) associated with the stream.
        ///
        /// # Examples
        ///
        /// ```rust,no_run
        /// # let stream: s2n_quic::stream::Stream = todo!();
        /// #
        /// let connection = stream.connection();
        ///
        /// println!("The stream's connection id is: {}", connection.id());
        /// ```
        #[inline]
        pub fn connection(&self) -> $crate::connection::Handle {
            let $self = self;
            $convert
        }
    };
}

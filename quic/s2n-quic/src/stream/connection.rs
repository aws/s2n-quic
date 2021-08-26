// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_connection_api {
    (| $self:ident | $convert:expr) => {
        /// Returns the [`Handle`] associated with the [`Stream`]
        ///
        /// # Examples
        ///
        /// ```rust
        /// // TODO
        /// ```
        pub fn connection(&self) -> $crate::connection::Handle {
            let $self = self;
            $convert
        }
    };
}

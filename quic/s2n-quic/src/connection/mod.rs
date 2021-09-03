// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_quic_transport::connection::Connection as Inner;

#[macro_use]
mod acceptor;
#[macro_use]
mod handle;

pub use acceptor::*;
pub use handle::*;
pub use s2n_quic_core::connection::Error;

pub type Result<T, E = Error> = core::result::Result<T, E>;

pub struct Connection(Inner);

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Connection {
    #[inline]
    pub(crate) const fn new(inner: Inner) -> Self {
        Self(inner)
    }

    impl_accept_api!();
    impl_handle_api!(|handle, call| call!(handle));

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    #[inline]
    pub fn split(self) -> (Handle, StreamAcceptor) {
        let handle = Handle(self.0.clone());
        let acceptor = StreamAcceptor(self.0);
        (handle, acceptor)
    }
}

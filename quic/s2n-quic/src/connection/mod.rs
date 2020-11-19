use core::fmt;
use s2n_quic_transport::connection::Connection as Inner;

#[macro_use]
mod acceptor;
#[macro_use]
mod handle;

pub use acceptor::*;
pub use handle::*;
pub use s2n_quic_core::connection::Error;

pub type Result<T> = core::result::Result<T, Error>;

pub struct Connection(Inner);

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum HandshakeStatus {
    /// The handshake has not completed
    Pending,

    /// The handshake has finished
    Complete,
}

impl Connection {
    pub(crate) const fn new(inner: Inner) -> Self {
        Self(inner)
    }

    impl_acceptor_api!(|handle, call| call!(handle));
    impl_handle_api!(|handle, call| call!(handle));

    /// TODO
    ///
    /// # Examples
    ///
    /// ```rust
    /// // TODO
    /// ```
    pub fn split(self) -> (Handle, StreamAcceptor) {
        todo!()
    }
}

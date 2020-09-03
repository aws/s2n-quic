pub mod metric;

#[macro_use]
mod acceptor;
#[macro_use]
mod handle;

pub use acceptor::*;
pub use handle::*;
pub use metric::Metric;
pub use s2n_quic_core::connection::Error;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct Connection {
    // TODO
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

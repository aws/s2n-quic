use s2n_quic_core::inet::SocketAddressV4;

pub use s2n_quic_core::path::Tuple as Handle;

#[derive(Clone, Debug)]
pub struct Controller {
    handle: Handle,
}

impl Controller {
    #[inline]
    pub fn server() -> Self {
        let local_address = SocketAddressV4::new([127, 0, 0, 1], 4433);
        let remote_address = SocketAddressV4::new([127, 0, 0, 2], 4433);

        let local_address = local_address.into();
        let remote_address = remote_address.into();

        let handle = Handle {
            local_address,
            remote_address,
        };
        Self { handle }
    }

    #[inline]
    pub fn client() -> Self {
        let mut v = Self::server();
        let remote = v.handle.local_address.0;
        let local = v.handle.remote_address.0;
        v.handle.remote_address = remote.into();
        v.handle.local_address = local.into();
        v
    }
}

impl super::Controller for Controller {
    type Handle = Handle;

    #[inline]
    fn handle(&self) -> &Self::Handle {
        &self.handle
    }
}

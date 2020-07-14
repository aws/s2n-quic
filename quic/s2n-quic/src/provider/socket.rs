use std::{
    io::Error as IOError,
    net::{ToSocketAddrs, UdpSocket},
};

/// Provides Socket support for an endpoint
pub trait Provider {
    // TODO
}

impl Provider for usize {
    // add code here
}

// TODO enable with specialization
// impl<Addr: ToSocketAddrs> TryInto for Addr {
//     type Error = std::io::Error;
//     type Provider = usize;

//     fn try_into(self) -> Result<Self::Provider, Self::Error> {
//         let addrs = self.to_socket_addrs()?;
//         Ok(1)
//     }
// }

impl TryInto for &str {
    type Error = IOError;
    type Provider = usize;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        let _addrs = self.to_socket_addrs()?;
        Ok(0)
    }
}

impl TryInto for UdpSocket {
    type Error = IOError;
    type Provider = usize;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        todo!()
    }
}

impl_provider_utils!();

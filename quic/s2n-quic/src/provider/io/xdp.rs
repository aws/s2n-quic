// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides an implementation of the [`io::Provider`](crate::provider::io::Provider)
//! using [AF_XDP](https://www.kernel.org/doc/html/latest/networking/af_xdp.html) sockets.

use s2n_quic_core::{
    endpoint::Endpoint,
    inet::SocketAddress,
    io::{rx, tx},
};

pub use s2n_quic_platform::io::xdp::*;

impl<Rx, Tx> super::Provider for Provider<Rx, Tx>
where
    Rx: 'static + rx::Rx<PathHandle = PathHandle> + Send,
    Tx: 'static + tx::Tx<PathHandle = PathHandle> + Send,
{
    type PathHandle = PathHandle;
    type Error = std::io::Error;

    fn start<E: Endpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<SocketAddress, Self::Error> {
        let (_join_handle, local_addr) = Provider::start(self, endpoint)?;
        Ok(local_addr)
    }
}

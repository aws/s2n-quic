// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::socket::{self, Protocol};

/// The transport a stream runs over
pub(crate) enum Transport<A, B, C> {
    DcUdp(A),
    DcTcp(B),
    Tls(C),
}

impl<T, C> Transport<T, T, C> {
    #[inline]
    pub(crate) fn variant(sockets: &dyn socket::Application) -> fn(T) -> Self {
        match sockets.protocol() {
            Protocol::Udp => Self::DcUdp,
            Protocol::Tcp => Self::DcTcp,
            Protocol::Other(_) if sockets.features().is_reliable() => Self::DcTcp,
            Protocol::Other(_) => Self::DcUdp,
        }
    }
}

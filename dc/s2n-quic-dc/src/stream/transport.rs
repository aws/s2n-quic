// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::socket::{self, Protocol};

/// The transport a stream runs over
pub(crate) enum Transport<T> {
    DcUdp(T),
    DcTcp(T),
    Tls(T),
}

impl<T> Transport<T> {
    #[inline]
    pub(crate) fn variant(sockets: &dyn socket::Application, is_tls: bool) -> fn(T) -> Self {
        if is_tls {
            return Self::Tls;
        }

        match sockets.protocol() {
            Protocol::Udp => Self::DcUdp,
            Protocol::Tcp => Self::DcTcp,
            Protocol::Other(_) if sockets.features().is_reliable() => Self::DcTcp,
            Protocol::Other(_) => Self::DcUdp,
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    io::tx::{self, router},
    path::Handle as _,
    xdp::path,
};

/// Routes TX messages based on if the local address port is non-zero
///
/// This can be used for client IO providers that wish to send the initial packet over a standard
/// UDP socket in order to offload address resolution to the operating system.
#[derive(Default)]
pub struct Router(());

impl router::Router for Router {
    type Handle = path::Tuple;

    /// If the local port is 0 then forward to `AddressUnknown`. Otherwise forward to
    /// `AddressKnown`.
    #[inline]
    fn route<M, AddressKnown, AddressUnknown>(
        &mut self,
        message: M,
        address_known: &mut AddressKnown,
        address_unknown: &mut AddressUnknown,
    ) -> Result<tx::Outcome, tx::Error>
    where
        M: tx::Message<Handle = Self::Handle>,
        AddressKnown: tx::Queue<Handle = Self::Handle>,
        AddressUnknown: tx::Queue<Handle = Self::Handle>,
    {
        if message.path_handle().local_address().port() == 0 {
            address_unknown.push(message)
        } else {
            address_known.push(message)
        }
    }
}

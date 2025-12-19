// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{crypto::tls::TlsSession, dc, stateless_reset, transport};
use alloc::vec::Vec;

/// The `dc::Endpoint` trait provides a way to support dc functionality
pub trait Endpoint: 'static + Send {
    /// If enabled, a dc version will attempt to be negotiated and dc-specific frames
    /// will be processed. Otherwise, no dc version will be negotiated and dc-specific
    /// frames received will result in a connection error.
    const ENABLED: bool = true;

    type Path: Path;

    /// Called when a dc version has been negotiated for the given `ConnectionInfo`
    ///
    /// Return `None` if dc should not be used for this path
    fn new_path(&mut self, connection_info: &dc::ConnectionInfo) -> Option<Self::Path>;

    /// Called when a datagram arrives that cannot be decoded as a non-DC QUIC packet, and
    /// thus may contain a secret control packet
    ///
    /// Return `true` if a secret control packet was decoded from the datagram, `false` otherwise
    fn on_possible_secret_control_packet(
        &mut self,
        datagram_info: &dc::DatagramInfo,
        payload: &mut [u8],
    ) -> bool;

    /// Configure whether mtu_probing_complete_support transport parameter is supported.
    ///
    /// The default implementation doesn't change anything. This method is primarily used by tests.
    fn with_mtu_probing_complete_support(self, _mtu_probing_complete_support: bool) -> Self
    where
        Self: Sized,
    {
        self
    }

    /// Returns whether mtu_probing_complete_support is enabled.
    /// This value is default to true if dc is enabled.
    fn mtu_probing_complete_support(&self) -> bool {
        true
    }
}

/// A dc path
pub trait Path: 'static + Send {
    /// Called when path secrets are ready to be derived from the given `TlsSession`
    ///
    /// Returns the stateless reset tokens to include in a `DC_STATELESS_RESET_TOKENS`
    /// frame sent to the peer.
    fn on_path_secrets_ready(
        &mut self,
        session: &impl TlsSession,
    ) -> Result<Vec<stateless_reset::Token>, transport::Error>;

    /// Called when a `DC_STATELESS_RESET_TOKENS` frame has been received from the peer
    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    );

    /// Called when the peer has confirmed receipt of `DC_STATELESS_RESET_TOKENS`, either
    /// by the server sending back its own `DC_STATELESS_RESET_TOKENS` or by the client
    /// acknowledging the `DC_STATELESS_RESET_TOKENS` frame was received.
    fn on_dc_handshake_complete(&mut self);

    /// Called when the MTU has been updated for the path
    fn on_mtu_updated(&mut self, mtu: u16);
}

impl<P: Path> Path for Option<P> {
    #[inline]
    fn on_path_secrets_ready(
        &mut self,
        session: &impl TlsSession,
    ) -> Result<Vec<stateless_reset::Token>, transport::Error> {
        if let Some(path) = self {
            path.on_path_secrets_ready(session)
        } else {
            Ok(Vec::default())
        }
    }

    #[inline]
    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a stateless_reset::Token>,
    ) {
        if let Some(path) = self {
            path.on_peer_stateless_reset_tokens(stateless_reset_tokens)
        }
    }

    #[inline]
    fn on_dc_handshake_complete(&mut self) {
        if let Some(path) = self {
            path.on_dc_handshake_complete()
        }
    }

    #[inline]
    fn on_mtu_updated(&mut self, max_datagram_size: u16) {
        if let Some(path) = self {
            path.on_mtu_updated(max_datagram_size)
        }
    }
}

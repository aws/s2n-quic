// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{application, transport};
pub use crate::{frame::ConnectionClose, inet::SocketAddress};

/// Provides a hook for applications to rewrite CONNECTION_CLOSE frames
///
/// Implementations should take care to not leak potentially sensitive information
/// to peers. This includes removing `reason` fields and making error codes more general.
pub trait Formatter: 'static {
    /// Formats a transport error for use in 1-RTT (application data) packets
    fn format_transport_error(&self, context: &Context, error: transport::Error)
        -> ConnectionClose;

    /// Formats an application error for use in 1-RTT (application data) packets
    fn format_application_error(
        &self,
        context: &Context,
        error: application::Error,
    ) -> ConnectionClose;

    /// Formats a transport error for use in early (initial, handshake) packets
    fn format_early_transport_error(
        &self,
        context: &Context,
        error: transport::Error,
    ) -> ConnectionClose;

    /// Formats an application error for use in early (initial, handshake) packets
    fn format_early_application_error(
        &self,
        context: &Context,
        error: application::Error,
    ) -> ConnectionClose;
}

#[non_exhaustive]
#[derive(Debug)]
pub struct Context<'a> {
    pub remote_address: &'a SocketAddress,
}

impl<'a> Context<'a> {
    pub fn new(remote_address: &'a SocketAddress) -> Self {
        Self { remote_address }
    }
}

/// A formatter that passes errors through, unmodified
///
/// WARNING: This formatter should only be used in application development,
///          as it can leak potentially sensitive information to the peer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Development;

impl Formatter for Development {
    fn format_transport_error(
        &self,
        _context: &Context,
        error: transport::Error,
    ) -> ConnectionClose {
        error.into()
    }

    fn format_application_error(
        &self,
        _context: &Context,
        error: application::Error,
    ) -> ConnectionClose {
        error.into()
    }

    fn format_early_transport_error(
        &self,
        _context: &Context,
        error: transport::Error,
    ) -> ConnectionClose {
        error.into()
    }

    fn format_early_application_error(
        &self,
        _context: &Context,
        error: application::Error,
    ) -> ConnectionClose {
        error.into()
    }
}

/// A formatter that removes potentially sensitive information
///
/// The following is performed:
///
/// * Reasons and frame_types are hidden
/// * INTERNAL_ERROR is transformed into PROTOCOL_VIOLATION
/// * Application codes are hidden in early (initial, handshake) packets
#[derive(Clone, Copy, Debug, Default)]
pub struct Production;

impl Formatter for Production {
    fn format_transport_error(
        &self,
        _context: &Context,
        error: transport::Error,
    ) -> ConnectionClose {
        // rewrite internal errors as PROTOCOL_VIOLATION
        if error.code == transport::Error::INTERNAL_ERROR.code {
            return transport::Error::PROTOCOL_VIOLATION.into();
        }

        // only preserve the error code
        transport::Error::new(*error.code).into()
    }

    fn format_application_error(
        &self,
        _context: &Context,
        error: application::Error,
    ) -> ConnectionClose {
        error.into()
    }

    fn format_early_transport_error(
        &self,
        context: &Context,
        error: transport::Error,
    ) -> ConnectionClose {
        Self.format_transport_error(context, error)
    }

    fn format_early_application_error(
        &self,
        _context: &Context,
        _error: application::Error,
    ) -> ConnectionClose {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.3
        //# Sending a CONNECTION_CLOSE of type 0x1d in an Initial or Handshake
        //# packet could expose application state or be used to alter application
        //# state.  A CONNECTION_CLOSE of type 0x1d MUST be replaced by a
        //# CONNECTION_CLOSE of type 0x1c when sending the frame in Initial or
        //# Handshake packets.  Otherwise, information about the application
        //# state might be revealed.  Endpoints MUST clear the value of the
        //# Reason Phrase field and SHOULD use the APPLICATION_ERROR code when
        //# converting to a CONNECTION_CLOSE of type 0x1c.

        transport::Error::APPLICATION_ERROR.into()
    }
}

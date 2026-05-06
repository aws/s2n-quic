// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::connection_close_formatter::{ConnectionClose, Context, Formatter};
use s2n_quic_core::{application, transport};

/// A formatter that passes transport errors through transparently.
///
/// This differs from the default [`Production`](s2n_quic_core::connection::close::Production) formatter
/// by preserving specific TLS alert codes (e.g., `CERTIFICATE_UNKNOWN`) over the wire.
/// Unlike the [`Development`](s2n_quic_core::connection::close::Development) formatter
/// this does clear the Reason Phrase field for early closure to remain compliant with RFC 9000.
///
/// This formatter is safe to use in controlled environments where both peers are
/// generally expected to be trusted infrastructure.
#[derive(Clone, Copy, Debug, Default)]
pub struct TransparentTransport;

impl Formatter for TransparentTransport {
    fn format_transport_error(
        &self,
        _context: &Context,
        error: transport::Error,
    ) -> ConnectionClose<'_> {
        error.into()
    }

    fn format_application_error(
        &self,
        _context: &Context,
        error: application::Error,
    ) -> ConnectionClose<'_> {
        error.into()
    }

    fn format_early_transport_error(
        &self,
        context: &Context,
        error: transport::Error,
    ) -> ConnectionClose<'_> {
        Self.format_transport_error(context, error)
    }

    fn format_early_application_error(
        &self,
        _context: &Context,
        _error: application::Error,
    ) -> ConnectionClose<'_> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.2.3
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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rustls::Error;

pub fn reason(error: rustls::Error) -> &'static str {
    match error {
        Error::InappropriateMessage { .. } => "received unexpected message",
        Error::InappropriateHandshakeMessage { .. } => "received unexpected handshake message",
        Error::NoCertificatesPresented => "peer sent no certificates",
        Error::UnsupportedNameType => "unsupported name type",
        Error::DecryptError => "cannot decrypt peer's message",
        Error::EncryptError => "cannot encrypt local message",
        Error::AlertReceived(_) => "received fatal alert",
        Error::FailedToGetCurrentTime => "failed to get current time",
        Error::FailedToGetRandomBytes => "failed to get random bytes",
        Error::HandshakeNotComplete => "handshake not complete",
        Error::PeerSentOversizedRecord => "peer sent excess record size",
        Error::NoApplicationProtocol => "peer doesn't support any known protocol",
        Error::BadMaxFragmentSize => "bad max fragment size",
        Error::General(_) => "unexpected error",
        Error::InvalidMessage(_) => "invalid message received",
        Error::PeerIncompatible(_) => "peer doesn't support a protocol version/feature",
        Error::PeerMisbehaved(_) => "peer TLS protocol error",
        Error::InvalidCertificate(_) => "invalid certificate",
        Error::InvalidCertRevocationList(_) => "invalid crl",
        Error::Other(_) => "some other error occurred",
        // rustls may add a new variant in the future that breaks us so do a wildcard
        #[allow(unreachable_patterns)]
        _ => "unexpected error",
    }
}

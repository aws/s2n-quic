// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::cipher_suite::{
    HeaderProtectionKey, HeaderProtectionKeys, OneRttKey, PacketKey, PacketKeys,
};
use bytes::Bytes;
use core::{fmt, fmt::Debug, task::Poll};
use rustls::quic::{self, Connection};
use s2n_quic_core::{
    application::ServerName,
    crypto::{self, tls, tls::CipherSuite},
    transport,
};

pub struct Session {
    connection: Connection,
    rx_phase: HandshakePhase,
    tx_phase: HandshakePhase,
    emitted_zero_rtt_keys: bool,
    emitted_handshake_complete: bool,
    emitted_server_name: bool,
    emitted_application_protocol: bool,
    server_name: Option<ServerName>,
}

impl tls::TlsSession for Session {
    fn tls_exporter(
        &self,
        label: &[u8],
        context: &[u8],
        output: &mut [u8],
    ) -> Result<(), tls::TlsExportError> {
        match self
            .connection
            .export_keying_material(output, label, Some(context))
        {
            Ok(_) => Ok(()),
            Err(_) => Err(tls::TlsExportError::failure()),
        }
    }

    fn cipher_suite(&self) -> CipherSuite {
        if let Some(rustls_cipher_suite) = self.connection.negotiated_cipher_suite() {
            match rustls_cipher_suite.suite() {
                rustls::CipherSuite::TLS13_AES_128_GCM_SHA256 => {
                    CipherSuite::TLS_AES_128_GCM_SHA256
                }
                rustls::CipherSuite::TLS13_AES_256_GCM_SHA384 => {
                    CipherSuite::TLS_AES_256_GCM_SHA384
                }
                rustls::CipherSuite::TLS13_CHACHA20_POLY1305_SHA256 => {
                    CipherSuite::TLS_CHACHA20_POLY1305_SHA256
                }
                _ => CipherSuite::Unknown,
            }
        } else {
            CipherSuite::Unknown
        }
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Session")
            .field("rx_phase", &self.rx_phase)
            .field("tx_phase", &self.tx_phase)
            .finish()
    }
}

impl Session {
    pub fn new(connection: Connection, server_name: Option<ServerName>) -> Self {
        Self {
            connection,
            rx_phase: Default::default(),
            tx_phase: Default::default(),
            emitted_zero_rtt_keys: false,
            emitted_handshake_complete: false,
            emitted_server_name: false,
            emitted_application_protocol: false,
            server_name,
        }
    }

    fn receive(&mut self, crypto_data: &[u8]) -> Result<(), transport::Error> {
        self.connection
            .read_hs(crypto_data)
            .map_err(crate::error::reason)
            .map_err(|reason| {
                //= https://www.rfc-editor.org/rfc/rfc9001#section-4.8
                //# QUIC is only able to convey an alert level of "fatal".  In TLS 1.3,
                //# the only existing uses for the "warning" level are to signal
                //# connection close; see Section 6.1 of [TLS13].  As QUIC provides
                //# alternative mechanisms for connection termination and the TLS
                //# connection is only closed if an error is encountered, a QUIC endpoint
                //# MUST treat any alert from TLS as if it were at the "fatal" level.

                // According to the rustls docs, `alert` only returns fatal alerts:
                // > https://docs.rs/rustls/0.19.0/rustls/quic/trait.QuicExt#tymethod.get_alert
                // > Emit the TLS description code of a fatal alert, if one has arisen.

                self.connection
                    .alert()
                    .map(|alert| {
                        // Explicitly annotate the type to detect if rustls starts
                        // returning a large array
                        let code: [u8; 1] = alert.to_array();
                        let code = code[0];
                        tls::Error { code, reason }
                    })
                    .unwrap_or(tls::Error::INTERNAL_ERROR)
            })?;
        Ok(())
    }

    fn application_parameters(&self) -> Result<tls::ApplicationParameters, transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-8.2
        //# endpoints that
        //# receive ClientHello or EncryptedExtensions messages without the
        //# quic_transport_parameters extension MUST close the connection with an
        //# error of type 0x16d (equivalent to a fatal TLS missing_extension
        //# alert, see Section 4.8).
        let transport_parameters =
            self.connection.quic_transport_parameters().ok_or_else(|| {
                tls::Error::MISSING_EXTENSION.with_reason("Missing QUIC transport parameters")
            })?;

        Ok(tls::ApplicationParameters {
            transport_parameters,
        })
    }

    //= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
    //# Unless
    //# another mechanism is used for agreeing on an application protocol,
    //# endpoints MUST use ALPN for this purpose.
    //
    //= https://www.rfc-editor.org/rfc/rfc7301#section-3.1
    //# Client                                              Server
    //#
    //#    ClientHello                     -------->       ServerHello
    //#      (ALPN extension &                               (ALPN extension &
    //#       list of protocols)                              selected protocol)
    //#                                                    [ChangeCipherSpec]
    //#                                    <--------       Finished
    //#    [ChangeCipherSpec]
    //#    Finished                        -------->
    //#    Application Data                <------->       Application Data
    fn application_protocol(&self) -> Option<&[u8]> {
        self.connection.alpn_protocol()
    }

    fn server_name(&self) -> Option<ServerName> {
        match &self.connection {
            Connection::Client(_) => self.server_name.clone(),
            Connection::Server(server) => {
                server.server_name().map(|server_name| server_name.into())
            }
        }
    }

    fn zero_rtt_keys(&mut self) -> Option<quic::DirectionalKeys> {
        if self.emitted_zero_rtt_keys {
            return None;
        }

        let keys = self.connection.zero_rtt_keys()?;
        self.emitted_zero_rtt_keys = true;
        Some(keys)
    }

    /// Check and process TLS handshake complete.
    ///
    /// Upon TLS handshake complete, emit an event to notify the transport layer.
    fn poll_complete_handshake<C: tls::Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Poll<Result<(), transport::Error>> {
        if self.tx_phase == HandshakePhase::Application && !self.connection.is_handshaking() {
            // attempt to emit server_name and application_protocol events prior to completing the
            // handshake
            self.emit_events(context)?;

            // the handshake is complete!
            if !self.emitted_handshake_complete {
                self.rx_phase.transition();
                context.on_handshake_complete()?;
                context.on_tls_exporter_ready(self)?;
            }

            self.emitted_handshake_complete = true;
        }

        if self.emitted_handshake_complete {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_impl<C: tls::Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Poll<Result<(), transport::Error>> {
        // Tracks if we have attempted to receive data at least once
        let mut has_tried_receive = false;

        loop {
            let crypto_data = match self.rx_phase {
                HandshakePhase::Initial => context.receive_initial(None),
                HandshakePhase::Handshake => context.receive_handshake(None),
                HandshakePhase::Application => context.receive_application(None),
            };

            // receive anything in the incoming buffer
            if let Some(crypto_data) = crypto_data {
                self.receive(&crypto_data)?;
            } else if has_tried_receive {
                return self.poll_complete_handshake(context);
                // If there's nothing to receive then we're done for now
            }

            if let Poll::Ready(()) = self.poll_complete_handshake(context)? {
                return Poll::Ready(Ok(()));
            }

            // mark that we tried to receive some data so we know next time we loop
            // to bail if nothing changed
            has_tried_receive = true;

            // try to pull out the early secrets, if any
            if let Some(keys) = self.zero_rtt_keys() {
                let (key, header_key) = PacketKey::new(
                    keys,
                    s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_128_GCM_SHA256,
                );
                context.on_zero_rtt_keys(key, header_key, self.application_parameters()?)?;
            }

            loop {
                // make sure we can send data before pulling it out of rustls
                let can_send = match self.tx_phase {
                    HandshakePhase::Initial => context.can_send_initial(),
                    HandshakePhase::Handshake => context.can_send_handshake(),
                    HandshakePhase::Application => context.can_send_application(),
                };

                if !can_send {
                    break;
                }

                let mut transmission_buffer = vec![];

                let key_change = self.connection.write_hs(&mut transmission_buffer);

                // if we didn't upgrade the key or transmit anything then we're waiting for
                // more reads
                if key_change.is_none() && transmission_buffer.is_empty() {
                    break;
                }

                // fill the correct buffer according to the handshake phase
                match self.tx_phase {
                    HandshakePhase::Initial => context.send_initial(transmission_buffer.into()),
                    HandshakePhase::Handshake => context.send_handshake(transmission_buffer.into()),
                    HandshakePhase::Application => {
                        context.send_application(transmission_buffer.into())
                    }
                }

                if let Some(key_change) = key_change {
                    let cipher_suite = self
                        .connection
                        .negotiated_cipher_suite()
                        .expect("cipher_suite should be negotiated")
                        .suite();
                    match key_change {
                        quic::KeyChange::Handshake { keys } => {
                            let (key, header_key) = PacketKeys::new(keys, cipher_suite);

                            context.on_handshake_keys(key, header_key)?;

                            // Transition both phases to Handshake
                            self.tx_phase.transition();
                            self.rx_phase.transition();
                        }
                        quic::KeyChange::OneRtt { keys, next } => {
                            let (key, header_key) = OneRttKey::new(keys, next, cipher_suite);

                            let application_parameters = self.application_parameters()?;

                            context.on_one_rtt_keys(key, header_key, application_parameters)?;

                            // Transition the tx_phase to Application
                            // Note: the rx_phase is transitioned when the handshake is complete
                            self.tx_phase.transition();
                        }
                    }
                }
            }
        }
    }

    fn emit_events<C: tls::Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Result<(), transport::Error> {
        if !self.emitted_server_name {
            if let Some(server_name) = self.server_name() {
                context.on_server_name(server_name)?;
                self.emitted_server_name = true;
            }
        }
        if !self.emitted_application_protocol {
            if let Some(application_protocol) = self.application_protocol() {
                let application_protocol = Bytes::copy_from_slice(application_protocol);
                context.on_application_protocol(application_protocol)?;
                self.emitted_application_protocol = true;
            }
        }

        Ok(())
    }
}

impl crypto::CryptoSuite for Session {
    type HandshakeKey = PacketKeys;
    type HandshakeHeaderKey = HeaderProtectionKeys;
    type InitialKey = s2n_quic_crypto::initial::InitialKey;
    type InitialHeaderKey = s2n_quic_crypto::initial::InitialHeaderKey;
    type OneRttKey = OneRttKey;
    type OneRttHeaderKey = HeaderProtectionKeys;
    type ZeroRttKey = PacketKey;
    type ZeroRttHeaderKey = HeaderProtectionKey;
    type RetryKey = s2n_quic_crypto::retry::RetryKey;
}

impl tls::Session for Session {
    fn poll<C: tls::Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Poll<Result<(), transport::Error>> {
        let result = self.poll_impl(context);
        // attempt to emit server_name and application_protocol events prior to possibly
        // returning with an error
        self.emit_events(context)?;
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
enum HandshakePhase {
    Initial,
    Handshake,
    Application,
}

impl HandshakePhase {
    fn transition(&mut self) {
        *self = match self {
            Self::Initial => Self::Handshake,
            _ => Self::Application,
        };
    }
}

impl Default for HandshakePhase {
    fn default() -> Self {
        Self::Initial
    }
}

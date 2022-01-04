// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::ciphersuite::{
    HeaderProtectionKey, HeaderProtectionKeys, OneRttKey, PacketKey, PacketKeys,
};
use core::fmt;
use rustls::{
    quic::{self, QuicExt},
    Connection,
};
use s2n_quic_core::{
    application::Sni,
    crypto::{self, tls, CryptoError},
    transport,
};

pub struct Session {
    connection: Connection,
    rx_phase: HandshakePhase,
    tx_phase: HandshakePhase,
    emitted_zero_rtt_keys: bool,
    emitted_handshake_complete: bool,
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
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            rx_phase: Default::default(),
            tx_phase: Default::default(),
            emitted_zero_rtt_keys: false,
            emitted_handshake_complete: false,
        }
    }

    fn receive(&mut self, crypto_data: &[u8]) -> Result<(), transport::Error> {
        self.connection
            .read_hs(crypto_data)
            .map_err(crate::error::reason)
            .map_err(|reason| {
                //= https://www.rfc-editor.org/rfc/rfc9001.txt#4.8
                //# QUIC is only able to convey an alert level of "fatal".  In TLS 1.3,
                //# the only existing uses for the "warning" level are to signal
                //# connection close; see Section 6.1 of [TLS13].  As QUIC provides
                //# alternative mechanisms for connection termination and the TLS
                //# connection is only closed if an error is encountered, a QUIC endpoint
                //# MUST treat any alert from TLS as if it were at the "fatal" level.

                // According to the rustls docs, `alert` only returns fatal alerts:
                // > https://docs.rs/rustls/0.19.0/rustls/quic/trait.QuicExt.html#tymethod.get_alert
                // > Emit the TLS description code of a fatal alert, if one has arisen.

                self.connection
                    .alert()
                    .map(|alert| CryptoError {
                        code: alert.get_u8(),
                        reason,
                    })
                    .unwrap_or(CryptoError::INTERNAL_ERROR)
            })?;
        Ok(())
    }

    fn application_parameters(&self) -> Result<tls::ApplicationParameters, transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9001.txt#8.1
        //# Unless
        //# another mechanism is used for agreeing on an application protocol,
        //# endpoints MUST use ALPN for this purpose.
        let alpn_protocol = self.connection.alpn_protocol().ok_or_else(||
            //= https://www.rfc-editor.org/rfc/rfc9001.txt#8.1
            //# When using ALPN, endpoints MUST immediately close a connection (see
            //# Section 10.2 of [QUIC-TRANSPORT]) with a no_application_protocol TLS
            //# alert (QUIC error code 0x178; see Section 4.8) if an application
            //# protocol is not negotiated.

            //= https://www.rfc-editor.org/rfc/rfc9001.txt#8.1
            //# While [ALPN] only specifies that servers
            //# use this alert, QUIC clients MUST use error 0x178 to terminate a
            //# connection when ALPN negotiation fails.
            CryptoError::NO_APPLICATION_PROTOCOL.with_reason("Missing ALPN protocol"))?;

        //= https://www.rfc-editor.org/rfc/rfc9001.txt#8.2
        //# endpoints that
        //# receive ClientHello or EncryptedExtensions messages without the
        //# quic_transport_parameters extension MUST close the connection with an
        //# error of type 0x16d (equivalent to a fatal TLS missing_extension
        //# alert, see Section 4.8).
        let transport_parameters =
            self.connection.quic_transport_parameters().ok_or_else(|| {
                CryptoError::MISSING_EXTENSION.with_reason("Missing QUIC transport parameters")
            })?;

        let sni = self.sni();

        Ok(tls::ApplicationParameters {
            alpn_protocol,
            sni,
            transport_parameters,
        })
    }

    fn sni(&self) -> Option<Sni> {
        match &self.connection {
            // TODO return the original value
            Connection::Client(_) => None,
            Connection::Server(server) => server.sni_hostname().map(|sni| sni.into()),
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
    fn try_complete_handshake<C: tls::Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Result<(), transport::Error> {
        if self.tx_phase == HandshakePhase::Application && !self.connection.is_handshaking() {
            // the handshake is complete!
            if !self.emitted_handshake_complete {
                self.rx_phase.transition();
                context.on_handshake_complete()?;
            }

            self.emitted_handshake_complete = true;
        }

        Ok(())
    }
}

impl crypto::CryptoSuite for Session {
    type HandshakeKey = PacketKeys;
    type HandshakeHeaderKey = HeaderProtectionKeys;
    type InitialKey = s2n_quic_ring::initial::RingInitialKey;
    type InitialHeaderKey = s2n_quic_ring::initial::RingInitialHeaderKey;
    type OneRttKey = OneRttKey;
    type OneRttHeaderKey = HeaderProtectionKeys;
    type ZeroRttKey = PacketKey;
    type ZeroRttHeaderKey = HeaderProtectionKey;
    type RetryKey = s2n_quic_ring::retry::RingRetryKey;
}

impl tls::Session for Session {
    fn poll<C: tls::Context<Self>>(&mut self, context: &mut C) -> Result<(), transport::Error> {
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
                self.try_complete_handshake(context)?;

                // If there's nothing to receive then we're done for now
                return Ok(());
            }

            self.try_complete_handshake(context)?;
            if self.emitted_handshake_complete {
                return Ok(());
            }

            // mark that we tried to receive some data so we know next time we loop
            // to bail if nothing changed
            has_tried_receive = true;

            // try to pull out the early secrets, if any
            if let Some(keys) = self.zero_rtt_keys() {
                let (key, header_key) = PacketKey::new(keys);
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
                    match key_change {
                        quic::KeyChange::Handshake { keys } => {
                            let (key, header_key) = PacketKeys::new(keys);

                            context.on_handshake_keys(key, header_key)?;

                            // Transition both phases to Handshake
                            self.tx_phase.transition();
                            self.rx_phase.transition();
                        }
                        quic::KeyChange::OneRtt { keys, next } => {
                            let (key, header_key) = OneRttKey::new(keys, next);
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

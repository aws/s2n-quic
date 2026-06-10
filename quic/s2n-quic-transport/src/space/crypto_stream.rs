// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    sync::data_sender::{self, DataSender, OutgoingDataFlowController},
    transmission,
};
use s2n_quic_core::{
    ack, buffer::Reassembler, frame::crypto::CryptoRef, transport, varint::VarInt,
};

pub type TxCryptoStream = DataSender<CryptoFlowController, data_sender::writer::Crypto>;

/// Serializes and writes `Crypto` frames
#[derive(Debug, Default)]
pub struct CryptoFlowController {}

/// There is no control flow for crypto data
impl OutgoingDataFlowController for CryptoFlowController {
    fn acquire_flow_control_window(&mut self, _end_offset: VarInt) -> VarInt {
        VarInt::MAX
    }

    fn is_blocked(&self) -> bool {
        false
    }

    fn clear_blocked(&mut self) {}

    fn finish(&mut self) {}
}

#[derive(Debug)]
pub struct CryptoStream {
    pub tx: TxCryptoStream,
    pub rx: Reassembler,
    is_finished: bool,
}

const TX_MAX_BUFFER_CAPACITY: u32 = 4096;

/// Maximum number of bytes that may be buffered ahead of the read cursor in the
/// crypto receive buffer. RFC 9000 §7.5 requires at least 4096; we allow up to
/// 128 KiB to accommodate larger handshake messages (e.g. certificate chains)
/// while still bounding memory consumption against adversarial CRYPTO frames.
const MAX_CRYPTO_BUFFER_SIZE: u64 = 128 * 1024;

impl Default for CryptoStream {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoStream {
    pub fn new() -> Self {
        Self {
            tx: TxCryptoStream::new(Default::default(), TX_MAX_BUFFER_CAPACITY),
            rx: Reassembler::default(),
            is_finished: false,
        }
    }

    pub fn can_send(&self) -> bool {
        !self.is_finished && self.tx.available_buffer_space() > 0
    }

    pub fn finish(&mut self) -> Result<(), transport::Error> {
        self.is_finished = true;
        self.tx.finish();

        //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.3
        //# When TLS
        //# provides keys for a higher encryption level, if there is data from
        //# a previous encryption level that TLS has not consumed, this MUST
        //# be treated as a connection error of type PROTOCOL_VIOLATION.
        if self.rx.is_empty() {
            Ok(())
        } else {
            Err(transport::Error::PROTOCOL_VIOLATION)
        }
    }

    pub fn on_crypto_frame(&mut self, frame: CryptoRef) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.3
        //# *  If the packet is from a previously installed encryption level, it
        //# MUST NOT contain data that extends past the end of previously
        //# received data in that flow.  Implementations MUST treat any
        //# violations of this requirement as a connection error of type
        //# PROTOCOL_VIOLATION.

        if self.is_finished && frame.offset + frame.data.len() > self.rx.total_received_len() {
            return Err(transport::Error::PROTOCOL_VIOLATION);
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
        //# Implementations MUST support buffering at least 4096 bytes of data
        //# received in out-of-order CRYPTO frames.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
        //# Endpoints MAY choose to
        //# allow more data to be buffered during the handshake.

        // Enforce the buffer size limit required by RFC 9000 §7.5.
        // This bounds the maximum distance between the read cursor and the farthest
        // byte a peer can write, capping total Reassembler memory for the crypto stream.
        let end_offset = frame
            .offset
            .checked_add_usize(frame.data.len())
            .ok_or(transport::Error::CRYPTO_BUFFER_EXCEEDED)?;

        let buffered = end_offset.as_u64().saturating_sub(self.rx.consumed_len());
        if buffered > MAX_CRYPTO_BUFFER_SIZE {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
            //# If an endpoint does not expand its buffer, it MUST close
            //# the connection with a CRYPTO_BUFFER_EXCEEDED error code.
            return Err(transport::Error::CRYPTO_BUFFER_EXCEEDED);
        }

        self.rx.write_at(frame.offset, frame.data).map_err(|_| {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
            //# If an endpoint does not expand its buffer, it MUST close
            //# the connection with a CRYPTO_BUFFER_EXCEEDED error code.

            transport::Error::CRYPTO_BUFFER_EXCEEDED
        })?;

        Ok(())
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) {
        self.tx.on_packet_ack(ack_set);
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.tx.on_packet_loss(ack_set);
    }

    /// This method gets called when a Retry packet is processed.
    pub fn on_retry_packet(&mut self) {
        self.tx.on_all_lost();
    }
}

impl transmission::interest::Provider for CryptoStream {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.tx.transmission_interest(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::frame::crypto::Crypto;

    fn crypto_frame(offset: u64, len: usize) -> CryptoRef<'static> {
        // Use a static buffer large enough for any test frame
        static DATA: [u8; 1024] = [0xAA; 1024];
        Crypto {
            offset: VarInt::new(offset).unwrap(),
            data: &DATA[..len],
        }
    }

    #[test]
    fn frame_within_buffer_limit_is_accepted() {
        let mut stream = CryptoStream::new();
        // A frame well within the 128 KiB limit should succeed
        assert!(stream.on_crypto_frame(crypto_frame(0, 1024)).is_ok());
    }

    #[test]
    fn frame_at_exact_limit_is_accepted() {
        let mut stream = CryptoStream::new();
        // end_offset == MAX_CRYPTO_BUFFER_SIZE is exactly at the boundary (not exceeded)
        let frame = Crypto {
            offset: VarInt::new(MAX_CRYPTO_BUFFER_SIZE - 16).unwrap(),
            data: &[0u8; 16][..],
        };
        assert!(stream.on_crypto_frame(frame).is_ok());
    }

    #[test]
    fn frame_exceeding_buffer_limit_is_rejected() {
        let mut stream = CryptoStream::new();
        // end_offset = MAX_CRYPTO_BUFFER_SIZE + 1, which exceeds the limit
        let frame = Crypto {
            offset: VarInt::new(MAX_CRYPTO_BUFFER_SIZE).unwrap(),
            data: &[0u8; 1][..],
        };
        let err = stream.on_crypto_frame(frame).unwrap_err();
        assert_eq!(err.code, transport::Error::CRYPTO_BUFFER_EXCEEDED.code);
    }

    #[test]
    fn large_offset_with_consumed_data_is_accepted() {
        let mut stream = CryptoStream::new();
        // Write and consume some data first so consumed_len advances
        assert!(stream.on_crypto_frame(crypto_frame(0, 1024)).is_ok());
        // Pop the data to advance consumed_len
        assert!(stream.rx.pop().is_some());

        // Now consumed_len == 1024, so a frame at offset 1024 + MAX - 16
        // has buffered = (1024 + MAX - 16 + 16) - 1024 = MAX, which is at the limit
        let frame = Crypto {
            offset: VarInt::new(1024 + MAX_CRYPTO_BUFFER_SIZE - 16).unwrap(),
            data: &[0u8; 16][..],
        };
        assert!(stream.on_crypto_frame(frame).is_ok());
    }

    #[test]
    fn large_offset_without_consumed_data_is_rejected() {
        let mut stream = CryptoStream::new();
        // Without consuming anything, a frame slightly larger than the limit should fail
        let frame = Crypto {
            offset: VarInt::new(MAX_CRYPTO_BUFFER_SIZE + 1).unwrap(),
            data: &[0u8; 1][..],
        };
        let err = stream.on_crypto_frame(frame).unwrap_err();
        assert_eq!(err.code, transport::Error::CRYPTO_BUFFER_EXCEEDED.code);
    }

    #[test]
    fn multiple_frames_within_limit_are_accepted() {
        let mut stream = CryptoStream::new();
        // Send multiple in-order frames that together stay within the limit
        for i in 0..10 {
            assert!(stream.on_crypto_frame(crypto_frame(i * 1024, 1024)).is_ok());
        }
    }
}

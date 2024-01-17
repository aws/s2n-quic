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
        //= type=TODO
        //= tracking-issue=356
        //= feature=Crypto buffer limits
        //# Implementations MUST support buffering at least 4096 bytes of data
        //# received in out-of-order CRYPTO frames.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
        //= type=TODO
        //= tracking-issue=356
        //= feature=Crypto buffer limits
        //# Endpoints MAY choose to
        //# allow more data to be buffered during the handshake.

        //TODO we need to limit the buffer size here

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

use core::convert::TryInto;
use s2n_quic_core::{
    inet::DatagramInfo, io::tx, packet::ProtectedPacket, token, transport::error::TransportError,
};

pub struct Manager<V> {
    inflight_handshakes: usize,
    token_validator: V,
}

impl<V: token::Format> Manager<V> {
    pub fn new(token_validator: V) -> Self {
        Self {
            inflight_handshakes: 0,
            token_validator,
        }
    }

    pub fn on_packet(
        &mut self,
        datagram_info: &DatagramInfo,
        packet: &ProtectedPacket,
    ) -> Result<(), TransportError> {
        let packet = match packet {
            ProtectedPacket::Initial(packet) => packet,
            _ => {
                return Ok(());
            }
        };

        match self.token_validator.validate_token(
            &datagram_info.remote_address,
            &packet.destination_connection_id().try_into()?,
            &packet.source_connection_id().try_into()?,
            packet.token(),
        ) {
            _ => {
                // TODO
            }
        };
        Ok(())
    }

    pub fn on_transmit<Tx: tx::Queue>(&mut self, _queue: &mut Tx) {
        todo!()
    }

    pub fn on_handshake_start(&mut self) {
        self.inflight_handshakes += 1;
    }

    pub fn on_handshake_end(&mut self) {
        self.inflight_handshakes -= 1;
    }

    pub fn inflight_handshakes(&self) -> usize {
        self.inflight_handshakes
    }
}

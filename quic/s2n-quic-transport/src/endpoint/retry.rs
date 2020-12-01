use alloc::collections::VecDeque;
use s2n_codec::{EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    connection,
    crypto::RetryCrypto,
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet,
    path::MINIMUM_MTU,
    time, token,
};

#[derive(Debug)]
pub struct Dispatch {
    transmissions: VecDeque<Transmission>,
}

const DEFAULT_MAX_PEERS: usize = 1024;
impl Default for Dispatch {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PEERS)
    }
}

impl Dispatch {
    pub fn new(max_peers: usize) -> Self {
        Self {
            transmissions: VecDeque::with_capacity(max_peers),
        }
    }

    #[allow(dead_code)]
    pub fn queue<T: token::Format, C: RetryCrypto>(
        &mut self,
        datagram: &DatagramInfo,
        packet: &packet::initial::ProtectedInitial,
        token_format: &mut T,
        crypto: C,
    ) {
        let transmission = Transmission::new(datagram.remote_address, packet, token_format, crypto);
        self.transmissions.push_front(transmission);
    }

    pub fn on_transmit<Tx: tx::Queue>(&mut self, queue: &mut Tx) {
        while let Some(transmission) = self.transmissions.pop_front() {
            if queue.push(&transmission).is_err() {
                self.transmissions.push_front(transmission);
                return;
            }
        }
    }
}

#[derive(Debug)]
pub struct Transmission {
    remote_address: SocketAddress,
    packet: [u8; MINIMUM_MTU as usize],
    packet_len: usize,
}

impl Transmission {
    pub fn new<T: token::Format, C: RetryCrypto>(
        remote_address: SocketAddress,
        packet: &packet::initial::ProtectedInitial,
        token_format: &mut T,
        _tag_generator: C,
    ) -> Self {
        let mut packet_buf = [0u8; MINIMUM_MTU as usize];
        let retry_packet = packet::retry::Retry::from_initial(packet);
        let mut token_buf = [0u8; MINIMUM_MTU as usize];
        let _token = token_format.generate_retry_token(
            &remote_address,
            &connection::Id::try_from_bytes(retry_packet.destination_connection_id).unwrap(),
            &connection::Id::try_from_bytes(retry_packet.source_connection_id).unwrap(),
            &mut token_buf,
        );
        let pseudo_packet = packet::retry::PseudoRetry::new(
            retry_packet.destination_connection_id,
            retry_packet.tag,
            retry_packet.version,
            retry_packet.source_connection_id,
            retry_packet.destination_connection_id,
            retry_packet.retry_token,
        );
        let mut buffer = EncoderBuffer::new(&mut packet_buf);
        pseudo_packet.encode(&mut buffer);

        // TODO: generate the tag using the RetryCrypto generic
        // TODO: Populate transmission with the encoded packet and correct length
        // https://github.com/awslabs/s2n-quic/issues/260
        Self {
            remote_address,
            packet: [0; MINIMUM_MTU as usize],
            packet_len: 0,
        }
    }
}

impl tx::Message for &Transmission {
    fn remote_address(&mut self) -> SocketAddress {
        self.remote_address
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    fn delay(&mut self) -> time::Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let len = self.packet_len;
        buffer[..len].copy_from_slice(&self.packet[..len]);
        len
    }
}

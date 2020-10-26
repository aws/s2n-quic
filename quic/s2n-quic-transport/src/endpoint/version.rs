use crate::endpoint;
use alloc::collections::VecDeque;
use core::{convert::TryInto, marker::PhantomData, time::Duration};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    connection,
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    io::tx,
    packet::{version_negotiation::VersionNegotiation, ProtectedPacket},
    path::MINIMUM_MTU,
    transport::error::TransportError,
};

#[derive(Debug)]
pub struct Negotiator<C> {
    transmissions: VecDeque<Transmission>,
    config: PhantomData<C>,
}

const MAX_PEERS: usize = 1024;

const SUPPORTED_VERSIONS: &[u32] = &[
    0xff00_0020, // draft-32 (https://github.com/quicwg/base-drafts/wiki/20th-Implementation-Draft)
    0xff00_001f, // draft-31
    0xff00_001e, // draft-30
    0xff00_001d, // draft-29 (https://github.com/quicwg/base-drafts/wiki/19th-Implementation-Draft)
];

macro_rules! is_supported {
    ($packet:ident) => {
        SUPPORTED_VERSIONS
            .iter()
            .cloned()
            .any(|v| v == $packet.version)
    };
}

impl<C: endpoint::Config> Negotiator<C> {
    pub fn new() -> Self {
        Self {
            transmissions: if C::ENDPOINT_TYPE.is_server() {
                VecDeque::with_capacity(MAX_PEERS)
            } else {
                VecDeque::new()
            },
            config: PhantomData,
        }
    }

    pub fn on_packet(
        &mut self,
        datagram_info: &DatagramInfo,
        packet: &ProtectedPacket,
    ) -> Result<(), TransportError> {
        // always forward packets for clients on to connections
        if C::ENDPOINT_TYPE.is_client() {
            return Ok(());
        }

        let packet = match packet {
            ProtectedPacket::Initial(packet) => {
                if is_supported!(packet) {
                    return Ok(());
                }
                packet
            }
            ProtectedPacket::ZeroRTT(packet) => {
                if is_supported!(packet) {
                    return Ok(());
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6.1
                //# a server that is able to recognize packets as
                //# 0-RTT might choose not to send Version Negotiation packets in
                //# response to 0-RTT packets with the expectation that it will
                //# eventually receive an Initial packet.
                return Err(TransportError::NO_ERROR);
            }
            _ => return Ok(()),
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#6
        //# A server might not send a Version
        //# Negotiation packet if the datagram it receives is smaller than the
        //# minimum size specified in a different version;
        if datagram_info.payload_len < (MINIMUM_MTU as usize) {
            return Err(TransportError::NO_ERROR);
        }

        // store the peer's address if we're not at capacity
        if self.transmissions.len() != MAX_PEERS {
            self.transmissions.push_back(Transmission {
                remote_address: datagram_info.remote_address,
                destination_connection_id: packet.destination_connection_id().try_into()?,
                source_connection_id: packet.source_connection_id().try_into()?,
            });
        }

        Err(TransportError::NO_ERROR)
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
struct Transmission {
    remote_address: SocketAddress,
    destination_connection_id: connection::Id,
    source_connection_id: connection::Id,
}

impl tx::Message for &Transmission {
    fn remote_address(&mut self) -> SocketAddress {
        self.remote_address
    }

    fn ecn(&mut self) -> ExplicitCongestionNotification {
        Default::default()
    }

    fn delay(&mut self) -> Duration {
        Default::default()
    }

    fn ipv6_flow_label(&mut self) -> u32 {
        0
    }

    fn write_payload(&mut self, buffer: &mut [u8]) -> usize {
        let mut buffer = EncoderBuffer::new(buffer);
        VersionNegotiation {
            tag: 0,
            destination_connection_id: self.destination_connection_id.as_ref(),
            source_connection_id: self.source_connection_id.as_ref(),
            supported_versions: SupportedVersions,
        }
        .encode(&mut buffer);
        buffer.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct SupportedVersions;

impl EncoderValue for SupportedVersions {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        for version in SUPPORTED_VERSIONS {
            encoder.encode(version);
        }
    }
}

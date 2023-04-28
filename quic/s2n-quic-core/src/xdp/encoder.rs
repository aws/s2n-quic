// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::path;
use crate::{
    inet::{
        checksum::Checksum,
        ethernet::{self, EtherType},
        ip::{self, IpAddress},
        ipv4, ipv6, udp,
    },
    io::tx::{self, Message, PayloadBuffer},
};
use core::{hash::Hasher, mem::size_of};
use s2n_codec::{Encoder, EncoderBuffer};

/// The default TTL/Hop Limit for the packets
///
/// This value comes from the default value for Linux.
///
/// ```text
/// $ sudo sysctl net.ipv4.ip_default_ttl
/// net.ipv4.ip_default_ttl = 64
/// ```
const DEFAULT_TTL: u8 = 64;

pub struct State {
    ipv4_id_counter: u16,
    ipv4_checksum: bool,
    // stores a copy of Checksum so we don't have to probe the platform function every time
    cached_checksum: Checksum,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ipv4_id_counter: 0,
            ipv4_checksum: true,
            cached_checksum: Default::default(),
        }
    }
}

impl State {
    #[inline]
    pub fn set_checksum(&mut self, enabled: bool) -> &mut Self {
        self.ipv4_checksum = enabled;
        self
    }

    #[inline]
    fn ipv4_id(&mut self) -> u16 {
        let id = self.ipv4_id_counter;
        self.ipv4_id_counter = self.ipv4_id_counter.wrapping_add(1);
        id
    }

    #[inline]
    fn ipv4_checksum(&self) -> Option<Checksum> {
        if self.ipv4_checksum {
            Some(self.cached_checksum)
        } else {
            None
        }
    }
}

#[inline]
pub fn encode_packet<M: Message<Handle = path::Tuple>>(
    buffer: &mut EncoderBuffer,
    message: &mut M,
    state: &mut State,
) -> Result<(), tx::Error> {
    unsafe {
        assume!(
            buffer.remaining_capacity()
                > size_of::<ethernet::Header>()
                    + size_of::<ipv6::Header>().max(size_of::<ipv4::Header>())
                    + size_of::<udp::Header>(),
            "buffer too small"
        );
    }

    let path = message.path_handle();
    match (path.local_address.ip, path.remote_address.ip) {
        (IpAddress::Ipv4(local_ip), IpAddress::Ipv4(remote_ip)) => {
            buffer.encode(&ethernet::Header {
                destination: path.remote_address.mac,
                source: path.local_address.mac,
                ethertype: EtherType::IPV4,
            });

            encode_ipv4(buffer, local_ip, remote_ip, message, state)
        }
        (local_ip, remote_ip) => {
            buffer.encode(&ethernet::Header {
                destination: path.remote_address.mac,
                source: path.local_address.mac,
                ethertype: EtherType::IPV6,
            });

            // if either/both of the addresses are IPv6 then both need to be mapped
            let local_ip = local_ip.to_ipv6_mapped();
            let remote_ip = remote_ip.to_ipv6_mapped();

            encode_ipv6(buffer, local_ip, remote_ip, message, state)
        }
    }
}

#[inline]
fn encode_ipv4<M: Message<Handle = path::Tuple>>(
    buffer: &mut EncoderBuffer,
    local_ip: ipv4::IpV4Address,
    remote_ip: ipv4::IpV4Address,
    message: &mut M,
    state: &mut State,
) -> Result<(), tx::Error> {
    const HEADER_LEN: u16 = (size_of::<ipv4::Header>() + size_of::<udp::Header>()) as _;

    let checksum = state.ipv4_checksum();

    let mut outcome = encode_payload(buffer, message, HEADER_LEN, checksum)?;

    buffer.write_zerocopy(|header: &mut ipv4::Header| {
        header.vihl_mut().set_version(4).set_header_len(5);
        header.tos_mut().set_dscp(0).set_ecn(message.ecn());
        header
            .flag_fragment_mut()
            .set_reserved(false)
            .set_dont_fragment(true)
            .set_more_fragments(false)
            .set_fragment_offset(0);
        header.id.set(state.ipv4_id());
        header.total_len_mut().set(HEADER_LEN + outcome.len);
        *header.ttl_mut() = DEFAULT_TTL;
        // set the checksum to zero for the initial pass
        header.checksum_mut().set(0);
        *header.protocol_mut() = ip::Protocol::UDP;
        *header.source_mut() = local_ip;
        *header.destination_mut() = remote_ip;

        // calculate the IPv4 header checksum
        {
            let mut checksum = state.cached_checksum;
            checksum.write(header.as_bytes());
            header.checksum_mut().set(checksum.finish());
        }

        // NOTE: duvet doesn't know how to parse this RFC since it doesn't follow more modern formatting
        //# https://www.rfc-editor.org/rfc/rfc768#Fields
        //# The pseudo  header  conceptually prefixed to the UDP header contains the
        //# source  address,  the destination  address,  the protocol,  and the  UDP
        //# length.   This information gives protection against misrouted datagrams.
        //# This checksum procedure is the same as is used in TCP.
        //#
        //#                  0      7 8     15 16    23 24    31
        //#                 +--------+--------+--------+--------+
        //#                 |          source address           |
        //#                 +--------+--------+--------+--------+
        //#                 |        destination address        |
        //#                 +--------+--------+--------+--------+
        //#                 |  zero  |protocol|   UDP length    |
        //#                 +--------+--------+--------+--------+
        if let Some(checksum) = outcome.checksum.as_mut() {
            // the addresses start at byte offset 12 in the header
            checksum.write(&header.as_bytes()[12..]);

            let payload_len = outcome.len + size_of::<udp::Header>() as u16;
            let payload_len = payload_len.to_be_bytes();

            let parts = [0, ip::Protocol::UDP.id, payload_len[0], payload_len[1]];

            checksum.write(&parts);
        }
    });

    encode_udp(buffer, outcome, message, state);

    Ok(())
}

#[inline]
fn encode_ipv6<M: Message<Handle = path::Tuple>>(
    buffer: &mut EncoderBuffer,
    local_ip: ipv6::IpV6Address,
    remote_ip: ipv6::IpV6Address,
    message: &mut M,
    state: &mut State,
) -> Result<(), tx::Error> {
    const HEADER_LEN: u16 = (size_of::<ipv6::Header>() + size_of::<udp::Header>()) as _;

    // Ipv6 checksums are required
    let checksum = Some(state.cached_checksum);

    let mut outcome = encode_payload(buffer, message, HEADER_LEN, checksum)?;

    buffer.write_zerocopy(|header: &mut ipv6::Header| {
        let payload_len = size_of::<udp::Header>() as u16 + outcome.len;

        header
            .vtcfl_mut()
            .set_version(6)
            .set_dscp(0)
            .set_ecn(message.ecn())
            .set_flow_label(message.ipv6_flow_label());
        header.payload_len_mut().set(payload_len);
        *header.next_header_mut() = ip::Protocol::UDP;
        *header.hop_limit_mut() = DEFAULT_TTL;
        *header.source_mut() = local_ip;
        *header.destination_mut() = remote_ip;

        //= https://www.rfc-editor.org/rfc/rfc2460#section-8.1
        //# Any transport or other upper-layer protocol that includes the
        //# addresses from the IP header in its checksum computation must be
        //# modified for use over IPv6, to include the 128-bit IPv6 addresses
        //# instead of 32-bit IPv4 addresses.  In particular, the following
        //# illustration shows the TCP and UDP "pseudo-header" for IPv6:
        //#
        //# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
        //# |                                                               |
        //# +                                                               +
        //# |                                                               |
        //# +                         Source Address                        +
        //# |                                                               |
        //# +                                                               +
        //# |                                                               |
        //# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
        //# |                                                               |
        //# +                                                               +
        //# |                                                               |
        //# +                      Destination Address                      +
        //# |                                                               |
        //# +                                                               +
        //# |                                                               |
        //# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
        //# |                   Upper-Layer Packet Length                   |
        //# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
        //# |                      zero                     |  Next Header  |
        //# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
        if let Some(checksum) = outcome.checksum.as_mut() {
            // the addresses start at byte offset 8 in the header
            checksum.write(&header.as_bytes()[8..]);

            let mut parts = [0; 8];
            parts[..4].copy_from_slice(&(payload_len as u32).to_be_bytes());
            parts[7] = ip::Protocol::UDP.id;

            checksum.write(&parts);
        }
    });

    encode_udp(buffer, outcome, message, state);

    Ok(())
}

#[inline]
fn encode_udp<M: Message<Handle = path::Tuple>>(
    buffer: &mut EncoderBuffer,
    outcome: PayloadOutcome,
    message: &mut M,
    _state: &mut State,
) {
    let path = message.path_handle();

    buffer.write_zerocopy(|header: &mut udp::Header| {
        header.source_mut().set(path.local_address.port);
        header.destination_mut().set(path.remote_address.port);
        // the length includes the UDP header
        header
            .len_mut()
            .set(size_of::<udp::Header>() as u16 + outcome.len);
        // initialize the checksum to 0
        header.checksum_mut().set(0);

        // write the checksum after we've written the header
        if let Some(mut checksum) = outcome.checksum {
            checksum.write(header.as_bytes());
            header.checksum_mut().set(checksum.finish());
        }
    });

    unsafe {
        assume!(
            buffer.remaining_capacity() >= outcome.len as usize,
            "buffer too small"
        );
    }

    // forward the buffer cursor to the end of the payload
    buffer.advance_position(outcome.len as _);
}

#[inline]
fn encode_payload<M: Message<Handle = path::Tuple>>(
    buffer: &mut EncoderBuffer,
    message: &mut M,
    header_size: u16,
    checksum: Option<Checksum>,
) -> Result<PayloadOutcome, tx::Error> {
    let header_position = buffer.len();
    buffer.advance_position(header_size as usize);

    let max_len = buffer
        .remaining_capacity()
        .min((u16::MAX - header_size) as usize);

    let mut outcome = PayloadOutcome { len: 0, checksum };

    unsafe {
        assume!(
            buffer.capacity() >= buffer.len(),
            "encoder cursors should be correct"
        );
    }
    let (_headers, payload) = buffer.split_mut();
    let payload = &mut payload[..max_len];
    {
        let payload = PayloadBuffer::new(payload);
        outcome.len = message.write_payload(payload, 0)? as u16;

        debug_assert!(outcome.len as usize <= max_len, "write exceeded max length");
    }

    if let Some(checksum) = outcome.checksum.as_mut() {
        unsafe {
            assume!(payload.len() >= outcome.len as usize);
        }
        checksum.write_padded(&payload[..outcome.len as usize]);
    }

    buffer.set_position(header_position);

    Ok(outcome)
}

#[derive(Clone, Copy, Debug, Default)]
struct PayloadOutcome {
    len: u16,
    checksum: Option<Checksum>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inet::ExplicitCongestionNotification, path::Handle};
    use bolero::{check, generator::*};
    use s2n_codec::DecoderBufferMut;

    #[derive(Debug, TypeGenerator)]
    pub struct Message {
        path: path::Tuple,
        ecn: ExplicitCongestionNotification,
        ipv4_id: u16,
        ipv4_checksum: bool,
        ipv6_flow_label: u32,
        payload: Vec<u8>,
    }

    impl<'a> tx::Message for &'a Message {
        type Handle = path::Tuple;

        fn path_handle(&self) -> &Self::Handle {
            &self.path
        }

        fn ecn(&mut self) -> ExplicitCongestionNotification {
            self.ecn
        }

        fn delay(&mut self) -> core::time::Duration {
            Default::default()
        }

        fn ipv6_flow_label(&mut self) -> u32 {
            self.ipv6_flow_label
        }

        fn can_gso(&self, _: usize, _: usize) -> bool {
            true
        }

        fn write_payload(
            &mut self,
            mut buffer: PayloadBuffer,
            _gso_offset: usize,
        ) -> Result<usize, tx::Error> {
            buffer.write(&self.payload)
        }
    }

    #[test]
    fn round_trip() {
        check!().with_type().for_each(|mut message: &Message| {
            let mut buffer = [0u8; 1500];
            let mut state = State {
                ipv4_id_counter: message.ipv4_id,
                ipv4_checksum: message.ipv4_checksum,
                cached_checksum: Checksum::default(),
            };

            let mut encoder = EncoderBuffer::new(&mut buffer);

            if encode_packet(&mut encoder, &mut message, &mut state).is_err() {
                return;
            }

            let (mut header, payload) =
                crate::xdp::decoder::decode_packet(DecoderBufferMut::new(&mut buffer))
                    .unwrap()
                    .unwrap();

            header.path.swap();

            assert!(Handle::eq(&header.path, &message.path));
            assert_eq!(header.ecn, message.ecn);
            assert_eq!(payload.into_less_safe_slice(), &message.payload);
        });
    }
}

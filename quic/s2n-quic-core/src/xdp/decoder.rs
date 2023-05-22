// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{bpf::Decoder, path};
use crate::inet::{
    datagram,
    ethernet::{self, EtherType},
    ip, ipv4, ipv6, udp,
};
use s2n_codec::DecoderError;

pub type Result<D = ()> = core::result::Result<Option<D>, DecoderError>;

pub trait EventHandler: Sized {
    #[inline(always)]
    fn decode_packet<'a, D: Decoder<'a>>(&mut self, buffer: D) -> Result<D> {
        decode_packet_with_event(buffer, self)
    }

    #[inline(always)]
    fn on_ethernet_header(&mut self, header: &ethernet::Header) -> Result {
        let _ = header;
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_ipv4_header(&mut self, header: &ipv4::Header) -> Result {
        let _ = header;
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_ipv6_header(&mut self, header: &ipv6::Header) -> Result {
        let _ = header;
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_udp_header(&mut self, header: &udp::Header) -> Result {
        let _ = header;
        Ok(Some(()))
    }
}

impl EventHandler for () {}

impl EventHandler for path::Tuple {
    #[inline(always)]
    fn on_ethernet_header(&mut self, header: &ethernet::Header) -> Result {
        self.remote_address.mac = header.source;
        self.local_address.mac = header.destination;
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_ipv4_header(&mut self, header: &ipv4::Header) -> Result {
        self.remote_address.ip = header.source.into();
        self.local_address.ip = header.destination.into();
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_ipv6_header(&mut self, header: &ipv6::Header) -> Result {
        self.remote_address.ip = header.source.into();
        self.local_address.ip = header.destination.into();
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_udp_header(&mut self, header: &udp::Header) -> Result {
        self.remote_address.port = header.source.get();
        self.local_address.port = header.destination.get();
        Ok(Some(()))
    }
}

impl<P: EventHandler> EventHandler for datagram::Header<P> {
    #[inline(always)]
    fn on_ethernet_header(&mut self, header: &ethernet::Header) -> Result {
        self.path.on_ethernet_header(header)
    }

    #[inline(always)]
    fn on_ipv4_header(&mut self, header: &ipv4::Header) -> Result {
        self.path.on_ipv4_header(header)?;
        self.ecn = header.tos().ecn();
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_ipv6_header(&mut self, header: &ipv6::Header) -> Result {
        self.path.on_ipv6_header(header)?;
        self.ecn = header.vtcfl().ecn();
        Ok(Some(()))
    }

    #[inline(always)]
    fn on_udp_header(&mut self, header: &udp::Header) -> Result {
        self.path.on_udp_header(header)
    }
}

/// Decodes a path tuple and payload from a raw packet
#[inline(always)]
pub fn decode_packet<'a, D: Decoder<'a>>(
    buffer: D,
) -> core::result::Result<Option<(datagram::Header<path::Tuple>, D)>, DecoderError> {
    let mut header = datagram::Header {
        path: path::Tuple::UNSPECIFIED,
        ecn: Default::default(),
    };
    match decode_packet_with_event(buffer, &mut header)? {
        Some(buffer) => Ok(Some((header, buffer))),
        None => Ok(None),
    }
}

/// Decodes a path tuple and payload from a raw packet
#[inline(always)]
pub fn decode_packet_with_event<'a, D: Decoder<'a>, E: EventHandler>(
    buffer: D,
    events: &mut E,
) -> Result<D> {
    let (header, buffer) = buffer.decode::<&ethernet::Header>()?;

    if events.on_ethernet_header(header)?.is_none() {
        return Ok(None);
    }

    match *header.ethertype() {
        EtherType::IPV4 => decode_ipv4(buffer, events),
        EtherType::IPV6 => decode_ipv6(buffer, events),
        // pass the packet on to the OS network stack if we don't understand it
        _ => Ok(None),
    }
}

#[inline(always)]
fn decode_ipv4<'a, D: Decoder<'a>, E: EventHandler>(buffer: D, events: &mut E) -> Result<D> {
    let (header, buffer) = buffer.decode::<&ipv4::Header>()?;

    if events.on_ipv4_header(header)?.is_none() {
        return Ok(None);
    }

    let protocol = header.protocol();

    //= https://www.rfc-editor.org/rfc/rfc791#section-3.1
    //# IHL:  4 bits
    //#
    //# Internet Header Length is the length of the internet header in 32
    //# bit words, and thus points to the beginning of the data.  Note that
    //# the minimum value for a correct header is 5.

    // subtract the fixed header size
    let count_without_header = header
        .vihl()
        .header_len()
        .checked_sub(5)
        .ok_or(DecoderError::InvariantViolation("invalid IPv4 IHL value"))?;

    // skip the options and go to the actual payload
    let options_len = count_without_header as usize * (32 / 8);
    let (_options, buffer) = buffer.decode_slice(options_len)?;

    parse_ip_protocol(protocol, buffer, events)
}

#[inline(always)]
fn decode_ipv6<'a, D: Decoder<'a>, E: EventHandler>(buffer: D, events: &mut E) -> Result<D> {
    let (header, buffer) = buffer.decode::<&ipv6::Header>()?;

    if events.on_ipv6_header(header)?.is_none() {
        return Ok(None);
    }

    let protocol = header.next_header();

    // TODO parse Hop-by-hop/Options headers, for now we'll just forward the packet on to the OS

    parse_ip_protocol(protocol, buffer, events)
}

#[inline]
fn parse_ip_protocol<'a, D: Decoder<'a>, E: EventHandler>(
    protocol: &ip::Protocol,
    buffer: D,
    events: &mut E,
) -> Result<D> {
    match *protocol {
        ip::Protocol::UDP | ip::Protocol::UDPLITE => parse_udp(buffer, events),
        // pass the packet on to the OS network stack if we don't understand it
        _ => Ok(None),
    }
}

#[inline(always)]
fn parse_udp<'a, D: Decoder<'a>, E: EventHandler>(buffer: D, events: &mut E) -> Result<D> {
    let (header, buffer) = buffer.decode::<&udp::Header>()?;

    if events.on_udp_header(header)?.is_none() {
        return Ok(None);
    }

    // NOTE: duvet doesn't know how to parse this RFC since it doesn't follow more modern formatting
    //# https://www.rfc-editor.org/rfc/rfc768
    //# Length  is the length  in octets  of this user datagram  including  this
    //# header  and the data.   (This  means  the minimum value of the length is
    //# eight.)
    let total_len = header.len().get();
    let payload_len = total_len
        .checked_sub(8)
        .ok_or(DecoderError::InvariantViolation("invalid UDP length"))?;
    let (udp_payload, _remaining) = buffer.decode_slice(payload_len as usize)?;

    Ok(Some(udp_payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    // Tests to ensure memory safety and no panics
    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(258), kani::solver(cadical))]
    fn decode_test() {
        check!().for_each(|bytes| {
            let buffer = s2n_codec::DecoderBuffer::new(bytes);
            let _ = decode_packet(buffer);
        });
    }
}

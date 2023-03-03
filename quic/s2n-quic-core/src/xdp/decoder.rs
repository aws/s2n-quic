// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{bpf::Decoder, path};
use crate::inet::{
    ethernet::{self, EtherType},
    ip, ipv4, ipv6, udp, SocketAddress,
};
use s2n_codec::DecoderError;

type Result<Addr, D> = core::result::Result<Option<(Addr, D)>, DecoderError>;

/// Decodes a path tuple and payload from a raw packet
#[inline(always)]
pub fn decode_packet<'a, D: Decoder<'a>>(buffer: D) -> Result<path::Tuple, D> {
    let (header, buffer) = buffer.decode::<&ethernet::Header>()?;

    let result = match *header.ethertype() {
        EtherType::IPV4 => decode_ipv4(buffer),
        EtherType::IPV6 => decode_ipv6(buffer),
        // pass the packet on to the OS network stack if we don't understand it
        _ => return Ok(None),
    }?;

    Ok(result.map(|(tuple, buffer)| {
        let remote_address = path::RemoteAddress {
            mac: *header.source(),
            ip: tuple.source.ip(),
            port: tuple.source.port(),
        };
        let local_address = path::LocalAddress {
            mac: *header.destination(),
            ip: tuple.destination.ip(),
            port: tuple.destination.port(),
        };
        let tuple = path::Tuple {
            remote_address,
            local_address,
        };
        (tuple, buffer)
    }))
}

#[inline(always)]
fn decode_ipv4<'a, D: Decoder<'a>>(buffer: D) -> Result<Tuple<SocketAddress>, D> {
    let (header, buffer) = buffer.decode::<&ipv4::Header>()?;
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

    Ok(parse_ip_protocol(protocol, buffer)?.map(|(ports, buffer)| {
        let source = header.source().with_port(ports.source).into();
        let destination = header.destination().with_port(ports.destination).into();
        let tuple = Tuple {
            source,
            destination,
        };
        (tuple, buffer)
    }))
}

#[inline(always)]
fn decode_ipv6<'a, D: Decoder<'a>>(buffer: D) -> Result<Tuple<SocketAddress>, D> {
    let (header, buffer) = buffer.decode::<&ipv6::Header>()?;
    let protocol = header.next_header();

    // TODO parse Hop-by-hop/Options headers, for now we'll just forward the packet on to the OS

    Ok(parse_ip_protocol(protocol, buffer)?.map(|(ports, buffer)| {
        let source = header.source().with_port(ports.source).into();
        let destination = header.destination().with_port(ports.destination).into();
        let tuple = Tuple {
            source,
            destination,
        };
        (tuple, buffer)
    }))
}

#[inline]
fn parse_ip_protocol<'a, D: Decoder<'a>>(
    protocol: &ip::Protocol,
    buffer: D,
) -> Result<Tuple<u16>, D> {
    match *protocol {
        ip::Protocol::UDP => parse_udp(buffer),
        // pass the packet on to the OS network stack if we don't understand it
        _ => Ok(None),
    }
}

#[inline(always)]
fn parse_udp<'a, D: Decoder<'a>>(buffer: D) -> Result<Tuple<u16>, D> {
    let (header, buffer) = buffer.decode::<&udp::Header>()?;

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

    let source = header.source().get();
    let destination = header.destination().get();

    let tuple = Tuple {
        source,
        destination,
    };

    Ok(Some((tuple, udp_payload)))
}

/// A generic tuple over an address type
#[derive(Clone, Copy, Debug)]
struct Tuple<Addr> {
    source: Addr,
    destination: Addr,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    // Tests to ensure memory safety and no panics
    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(258), kani::solver(kissat))]
    fn decode_test() {
        check!().for_each(|bytes| {
            let buffer = s2n_codec::DecoderBuffer::new(bytes);
            let _ = decode_packet(buffer);
        });
    }
}

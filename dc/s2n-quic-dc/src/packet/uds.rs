// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, DecoderError, Encoder};
use s2n_quic_core::inet::{SocketAddress, SocketAddressV4, SocketAddressV6};

pub mod decoder;
pub mod encoder;

/// The transport of a stream handed off over UDS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// The TCP socket is passed alongside the packet.
    Tcp,
    /// No file descriptor is passed; the receiver replies from its own UDP sockets.
    Udp,
}

const PROTOCOL_TCP: u8 = 0;
const PROTOCOL_UDP: u8 = 1;

const ADDRESS_NONE: u8 = 0;
const ADDRESS_V4: u8 = 4;
const ADDRESS_V6: u8 = 6;

impl From<Protocol> for u8 {
    #[inline]
    fn from(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Tcp => PROTOCOL_TCP,
            Protocol::Udp => PROTOCOL_UDP,
        }
    }
}

impl TryFrom<u8> for Protocol {
    type Error = &'static str;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            PROTOCOL_TCP => Ok(Self::Tcp),
            PROTOCOL_UDP => Ok(Self::Udp),
            _ => Err("Unsupported protocol"),
        }
    }
}

#[inline]
fn encode_address<E: Encoder>(encoder: &mut E, address: Option<&SocketAddress>) {
    match address {
        None => encoder.encode(&ADDRESS_NONE),
        Some(SocketAddress::IpV4(address)) => {
            encoder.encode(&ADDRESS_V4);
            encoder.encode(address);
        }
        Some(SocketAddress::IpV6(address)) => {
            encoder.encode(&ADDRESS_V6);
            encoder.encode(address);
        }
    }
}

#[inline]
fn decode_address(buffer: DecoderBufferMut) -> DecoderBufferMutResult<Option<SocketAddress>> {
    let (family, buffer) = buffer.decode::<u8>()?;
    match family {
        ADDRESS_NONE => Ok((None, buffer)),
        ADDRESS_V4 => {
            let (address, buffer) = buffer.decode::<SocketAddressV4>()?;
            Ok((Some(address.into()), buffer))
        }
        ADDRESS_V6 => {
            let (address, buffer) = buffer.decode::<SocketAddressV6>()?;
            Ok((Some(address.into()), buffer))
        }
        _ => Err(DecoderError::InvariantViolation(
            "Unsupported address family",
        )),
    }
}

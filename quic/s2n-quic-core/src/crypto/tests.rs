// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::{scatter, CryptoError, HeaderKey, HeaderProtectionMask, Key, ProtectedPayload},
    packet::number::{PacketNumber, PacketNumberSpace},
    varint::VarInt,
};
use bolero::{check, generator::*};
use core::mem::size_of;
use s2n_codec::{DecoderBufferMut, EncoderBuffer};
use std::convert::TryInto;

#[test]
#[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
fn round_trip() {
    check!()
        .with_generator((
            gen()
                .map_gen(VarInt::from_u32)
                .map_gen(|value| PacketNumberSpace::Initial.new_packet_number(value)),
            gen::<Vec<u8>>(),
        ))
        .for_each(|(largest_packet_number, input)| {
            let mut buffer = input.clone();

            if let Ok((packet_number, header_len)) =
                fuzz_unprotect(&mut buffer, *largest_packet_number)
            {
                fuzz_protect(
                    &mut buffer,
                    header_len,
                    *largest_packet_number,
                    packet_number,
                )
                .expect("protection should always work");

                assert_eq!(input, &buffer);
            }
        });
}

fn fuzz_unprotect(
    input: &mut [u8],
    largest_packet_number: PacketNumber,
) -> Result<(PacketNumber, usize), CryptoError> {
    let buffer = DecoderBufferMut::new(input);
    let header_len = {
        let peek = buffer.peek();
        let original_len = peek.len();
        let peek = peek.skip(1)?; // skip tag
        let peek = peek.skip_with_len_prefix::<u8>()?; // skip a variable len slice
        original_len - peek.len()
    };
    let (payload, _) = buffer.decode::<DecoderBufferMut>()?;
    let payload = ProtectedPayload::new(header_len, payload.into_less_safe_slice());

    let (truncated_packet_number, payload) =
        crate::crypto::unprotect(&FuzzCrypto, largest_packet_number.space(), payload)?;

    let packet_number = truncated_packet_number.expand(largest_packet_number);

    // make sure the packet number can be truncated and is canonical
    packet_number
        .truncate(largest_packet_number)
        .filter(|actual| truncated_packet_number.eq(actual))
        .ok_or(CryptoError::DECODE_ERROR)?;

    let (_header, _payload) = crate::crypto::decrypt(&FuzzCrypto, packet_number, payload)?;

    Ok((packet_number, header_len))
}

fn fuzz_protect(
    input: &mut [u8],
    header_len: usize,
    largest_packet_number: PacketNumber,
    packet_number: PacketNumber,
) -> Result<(), CryptoError> {
    let payload_len = input.len();
    let mut payload = EncoderBuffer::new(input);
    payload.set_position(payload_len);
    let payload = scatter::Buffer::new(payload);

    let truncated_packet_number = packet_number.truncate(largest_packet_number).unwrap();
    let packet_number_len = truncated_packet_number.len();

    let (payload, _remaining) = crate::crypto::encrypt(
        &FuzzCrypto,
        packet_number,
        packet_number_len,
        header_len,
        payload,
    )?;

    let _payload = crate::crypto::protect(&FuzzCrypto, payload)?;

    Ok(())
}

/// `FuzzCrypto` does not use any secrets which makes fuzzing all the wiring possible
struct FuzzCrypto;

/// `FuzzCrypto` uses the packet number as a single byte mask applied to the payload
impl Key for FuzzCrypto {
    fn decrypt(
        &self,
        packet_number: u64,
        _header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        let mask = packet_number as u8;
        for byte in payload.iter_mut() {
            *byte ^= mask;
        }
        Ok(())
    }

    fn encrypt<'a>(
        &self,
        packet_number: u64,
        _header: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result<(), CryptoError> {
        let payload = payload.flatten();
        let (payload, _) = payload.split_mut();

        let mask = packet_number as u8;
        for byte in payload.iter_mut() {
            *byte ^= mask;
        }

        Ok(())
    }

    fn tag_len(&self) -> usize {
        0
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        0
    }

    fn aead_integrity_limit(&self) -> u64 {
        0
    }

    fn cipher_suite(&self) -> crate::crypto::tls::CipherSuite {
        crate::crypto::tls::CipherSuite::Unknown
    }
}

/// `FuzzCrypto` uses the first 5 bytes of the payload as the protection mask
impl HeaderKey for FuzzCrypto {
    fn opening_header_protection_mask(&self, buffer: &[u8]) -> HeaderProtectionMask {
        buffer.try_into().unwrap()
    }

    fn opening_sample_len(&self) -> usize {
        size_of::<HeaderProtectionMask>()
    }

    fn sealing_header_protection_mask(&self, buffer: &[u8]) -> HeaderProtectionMask {
        buffer.try_into().unwrap()
    }

    fn sealing_sample_len(&self) -> usize {
        size_of::<HeaderProtectionMask>()
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    control,
    credentials::Credentials,
    crypto::seal,
    packet::{self, datagram::encoder},
};
use core::sync::atomic::{AtomicU64, Ordering};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{ensure, varint::VarInt};

#[derive(Clone, Copy, Debug)]
pub enum Error {
    PayloadTooLarge,
    PacketBufferTooSmall,
    PacketNumberExhaustion,
}

pub struct Sender<E> {
    encrypt_key: E,
    credentials: Credentials,
    packet_number: AtomicU64,
}

impl<E> Sender<E>
where
    E: seal::Application,
{
    #[inline]
    pub fn new(encrypt_key: E, credentials: Credentials) -> Self {
        Self {
            encrypt_key,
            credentials,
            packet_number: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn estimated_send_size(&self, cleartext_payload_len: usize) -> Option<usize> {
        let payload_len = packet::PayloadLen::try_from(cleartext_payload_len).ok()?;
        Some(encoder::estimate_len(
            VarInt::ZERO,
            None,
            VarInt::ZERO,
            payload_len,
            E::tag_len(&self.encrypt_key),
        ))
    }

    #[inline]
    pub fn send_into<C>(
        &self,
        control_port: &C,
        mut cleartext_payload: &[u8],
        encrypted_packet: &mut [u8],
    ) -> Result<usize, Error>
    where
        C: control::Controller,
    {
        let packet_number = self.packet_number.fetch_add(1, Ordering::Relaxed);
        let packet_number =
            VarInt::new(packet_number).map_err(|_| Error::PacketNumberExhaustion)?;

        let payload_len = packet::PayloadLen::try_from(cleartext_payload.len())
            .map_err(|_| Error::PayloadTooLarge)?;

        let estimated_packet_len = self
            .estimated_send_size(cleartext_payload.len())
            .ok_or(Error::PayloadTooLarge)?;

        // ensure the descriptor has enough capacity after MTU/allocation
        ensure!(
            encrypted_packet.len() >= estimated_packet_len,
            Err(Error::PacketBufferTooSmall)
        );

        let actual_packet_len = {
            let source_control_port = control_port.source_port();

            let encoder = EncoderBuffer::new(encrypted_packet);

            encoder::encode(
                encoder,
                source_control_port,
                Some(packet_number),
                None,
                VarInt::ZERO,
                &mut &[][..],
                &(),
                payload_len,
                &mut cleartext_payload,
                &self.encrypt_key,
                &self.credentials,
            )
        };

        Ok(actual_packet_len)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet::number::PacketNumber, time::Timestamp};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};

#[derive(Debug)]
pub struct Packet {
    pub number: PacketNumber,
    pub timestamp: Timestamp,
}

/// Trait which enables an application to intercept packets that are transmitted and received
pub trait Interceptor: 'static + Send {
    #[inline(always)]
    fn intercept_rx_packet<'a>(
        &mut self,
        packet: Packet,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        let _ = packet;
        payload
    }

    #[inline(always)]
    fn intercept_tx_packet<'a>(&mut self, packet: Packet, payload: &mut EncoderBuffer<'a>) {
        let _ = packet;
        let _ = payload;
    }
}

#[derive(Debug, Default)]
pub struct Disabled(());

impl Interceptor for Disabled {}

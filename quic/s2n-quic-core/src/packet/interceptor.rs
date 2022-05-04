// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event::api::Subject, havoc, packet::number::PacketNumber, time::Timestamp};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};

/// TODO add `non_exhaustive` once/if this feature is stable
#[derive(Debug)]
pub struct Packet {
    pub number: PacketNumber,
    pub timestamp: Timestamp,
}

/// Trait which enables an application to intercept packets that are transmitted and received
pub trait Interceptor: 'static + Send {
    #[inline(always)]
    fn intercept_rx_payload<'a>(
        &mut self,
        subject: Subject,
        packet: Packet,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        let _ = subject;
        let _ = packet;
        payload
    }

    #[inline(always)]
    fn intercept_tx_payload<'a>(
        &mut self,
        subject: Subject,
        packet: Packet,
        payload: &mut EncoderBuffer<'a>,
    ) {
        let _ = subject;
        let _ = packet;
        let _ = payload;
    }
}

#[derive(Debug, Default)]
pub struct Disabled(());

impl Interceptor for Disabled {}

#[derive(Debug, Default)]
pub struct Havoc<Rx, Tx, R>
where
    Rx: 'static + Send + havoc::Strategy,
    Tx: 'static + Send + havoc::Strategy,
    R: 'static + Send + havoc::Random,
{
    pub rx: Rx,
    pub tx: Tx,
    pub random: R,
}

impl<Rx, Tx, R> Interceptor for Havoc<Rx, Tx, R>
where
    Rx: 'static + Send + havoc::Strategy,
    Tx: 'static + Send + havoc::Strategy,
    R: 'static + Send + havoc::Random,
{
    #[inline]
    fn intercept_rx_payload<'a>(
        &mut self,
        _subject: Subject,
        _packet: Packet,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        let payload = payload.into_less_safe_slice();
        let len = payload.len();

        let len = {
            use s2n_codec::Encoder;
            let mut payload = EncoderBuffer::new(payload);
            payload.set_position(len);
            self.rx.havoc(&mut self.random, &mut payload);
            payload.len()
        };

        let payload = &mut payload[..len];

        DecoderBufferMut::new(payload)
    }

    #[inline]
    fn intercept_tx_payload<'a>(
        &mut self,
        _subject: Subject,
        _packet: Packet,
        payload: &mut EncoderBuffer<'a>,
    ) {
        self.tx.havoc(&mut self.random, payload);
    }
}

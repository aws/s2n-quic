// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet, stream::recv};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

pub trait Dispatch {
    #[inline(always)]
    fn tag_len(&self) -> usize {
        16
    }

    fn on_packet(
        &mut self,
        remote_addr: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        packet: packet::Packet,
    ) -> Result<(), recv::Error>;

    #[inline]
    fn on_datagram_segment(
        &mut self,
        remote_addr: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        segment: &mut [u8],
    ) -> Result<(), recv::Error> {
        let tag_len = self.tag_len();
        let segment_len = segment.len();
        let mut decoder = DecoderBufferMut::new(segment);

        while !decoder.is_empty() {
            let packet = match decoder.decode_parameterized(tag_len) {
                Ok((packet, remaining)) => {
                    decoder = remaining;
                    packet
                }
                Err(decoder_error) => {
                    // the packet was likely corrupted so log it and move on to the
                    // next segment
                    tracing::warn!(
                        %decoder_error,
                        segment_len
                    );
                    break;
                }
            };

            self.on_packet(remote_addr, ecn, packet)?;
        }

        Ok(())
    }
}

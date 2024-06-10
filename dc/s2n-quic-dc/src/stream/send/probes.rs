// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::too_many_arguments)]

use crate::{credentials, packet::stream};
use core::time::Duration;
use s2n_quic_core::{packet::number::PacketNumber, probe, varint::VarInt};

probe::define!(
    extern "probe" {
        /// Called when a control packet is received
        #[link_name = s2n_quic_dc__stream__send__control_packet]
        pub fn on_control_packet(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_number: VarInt,
            control_data_len: usize,
        );

        /// Called when a control packet is decrypted
        #[link_name = s2n_quic_dc__stream__send__control_packet_decrypted]
        pub fn on_control_packet_decrypted(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_number: VarInt,
            control_data_len: usize,
            valid: bool,
        );

        /// Called when a control packet was dropped due to being a duplicate
        #[link_name = s2n_quic_dc__stream__send__control_packet_decrypted]
        pub fn on_control_packet_duplicate(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_number: VarInt,
            control_data_len: usize,
        );

        /// Called when a packet was ACK'd
        #[link_name = s2n_quic_dc__stream__send__packet_ack]
        pub fn on_packet_ack(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_space: stream::PacketSpace,
            packet_number: u64,
            packet_len: u16,
            stream_offset: VarInt,
            payload_len: u16,
            lifetime: Duration,
        );

        /// Called when a packet was lost
        #[link_name = s2n_quic_dc__stream__send__packet_lost]
        pub fn on_packet_lost(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_space: stream::PacketSpace,
            packet_number: u64,
            packet_len: u16,
            stream_offset: VarInt,
            payload_len: u16,
            lifetime: Duration,
            needs_retransmission: bool,
        );

        /// Called when a packet was ACK'd
        #[link_name = s2n_quic_dc__stream__send__pto_backoff_reset]
        pub fn on_pto_backoff_reset(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            previous_value: u32,
        );

        /// Called when the PTO timer is armed
        #[link_name = s2n_quic_dc__stream__send__pto_armed]
        pub fn on_pto_armed(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            pto_period: Duration,
            pto_backoff: u32,
        );

        /// Called when a range of stream bytes are transmitted
        #[link_name = s2n_quic_dc__stream__send__transmit_stream]
        pub fn on_transmit_stream(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_space: stream::PacketSpace,
            packet_number: PacketNumber,
            stream_offset: VarInt,
            payload_len: u16,
            included_fin: bool,
            is_retransmission: bool,
        );

        /// Called when a range of stream bytes are transmitted as a probe
        #[link_name = s2n_quic_dc__stream__send__transmit_probe]
        pub fn on_transmit_probe(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_space: stream::PacketSpace,
            packet_number: PacketNumber,
            stream_offset: VarInt,
            payload_len: u16,
            included_fin: bool,
            is_retransmission: bool,
        );

        /// Called when a control packet is received
        #[link_name = s2n_quic_dc__stream__send__close]
        pub fn on_close(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_number: VarInt,
            error_code: VarInt,
        );
    }
);

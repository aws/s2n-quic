// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{credentials, packet::stream};
use s2n_quic_core::{packet::number::PacketNumber, probe, varint::VarInt};

probe::define!(
    extern "probe" {
        /// Called when a stream packet is received
        #[link_name = s2n_quic_dc__stream__recv__stream_packet]
        pub fn on_stream_packet(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            space: stream::PacketSpace,
            packet_number: VarInt,
            stream_offset: VarInt,
            payload_data_len: usize,
            included_fin: bool,
            is_retransmission: bool,
        );

        /// Called when a stream packet is decrypted
        #[link_name = s2n_quic_dc__stream__recv__stream_packet_decrypted]
        pub fn on_stream_packet_decrypted(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            space: stream::PacketSpace,
            packet_number: VarInt,
            stream_offset: VarInt,
            payload_data_len: usize,
            included_fin: bool,
            is_retransmission: bool,
            valid: bool,
        );

        /// Called when a range of ACKs are transmitted
        #[link_name = s2n_quic_dc__stream__recv__transmit_control]
        pub fn on_transmit_control(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            space: stream::PacketSpace,
            packet_number: VarInt,
            lowest_acked: PacketNumber,
            highest_acked: PacketNumber,
            gaps: usize,
        );

        /// Called when a CONNECTION_CLOSE is transmitted
        #[link_name = s2n_quic_dc__stream__recv__transmit_close]
        pub fn on_transmit_close(
            credential_id: credentials::Id,
            stream_id: stream::Id,
            packet_number: VarInt,
            code: VarInt,
        );
    }
);

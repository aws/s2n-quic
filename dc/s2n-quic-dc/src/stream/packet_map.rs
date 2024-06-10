// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub type Map<Data> = s2n_quic_core::packet::number::Map<SentPacketInfo<Data>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SentPacketInfo<Data> {
    pub data: Data,
    pub cc_info: crate::congestion::PacketInfo,
}

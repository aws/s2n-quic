// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::varint::VarInt;

pub fn iter(space: PacketNumberSpace) -> impl Iterator<Item = PacketNumber> {
    core::iter::successors(Some(space.new_packet_number(VarInt::from_u8(0))), |prev| {
        prev.next()
    })
}

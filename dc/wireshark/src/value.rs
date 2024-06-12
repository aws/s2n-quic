// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::Buffer, wireshark::Node};
use s2n_quic_core::varint::VarInt;
use s2n_quic_dc::packet;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Parsed<T> {
    // offset and length inside `tvb`
    pub offset: usize,
    pub len: usize,
    pub value: T,
}

impl<T> Parsed<T> {
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Parsed<U> {
        let value = map(self.value);
        Parsed {
            offset: self.offset,
            len: self.len,
            value,
        }
    }

    pub fn with<U>(self, value: U) -> Parsed<U> {
        self.map(|_| value)
    }
}

impl Parsed<packet::Tag> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u8(buffer, field, self.map(|v| v.into()))
    }
}

impl Parsed<packet::stream::Tag> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u8(buffer, field, self.map(|v| v.into()))
    }
}

impl Parsed<packet::control::Tag> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u8(buffer, field, self.map(|v| v.into()))
    }
}

impl Parsed<packet::datagram::Tag> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u8(buffer, field, self.map(|v| v.into()))
    }
}

impl Parsed<u64> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u64(buffer, field, *self)
    }
}

impl Parsed<VarInt> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u64(buffer, field, self.map(|v| v.as_u64()))
    }
}

impl Parsed<u32> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u32(buffer, field, *self)
    }
}

impl Parsed<u16> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_u16(buffer, field, *self)
    }
}

impl Parsed<core::time::Duration> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_duration(buffer, field, *self)
    }
}

impl Parsed<&'_ [u8]> {
    pub fn record<T: Node>(&self, buffer: &Buffer, tree: &mut T, field: i32) -> T::AddedItem {
        tree.add_slice(buffer, field, *self)
    }

    pub fn record_hidden<T: Node>(
        &self,
        buffer: &Buffer,
        tree: &mut T,
        field: i32,
    ) -> T::AddedItem {
        tree.add_slice_hidden(buffer, field, *self)
    }
}

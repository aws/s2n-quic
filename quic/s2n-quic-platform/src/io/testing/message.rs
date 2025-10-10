// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::{self, Message as MessageTrait};
use core::alloc::Layout;
use s2n_quic_core::{
    inet::{datagram, ExplicitCongestionNotification},
    io::tx,
    path,
};

/// A simple message type that holds an address and payload
///
/// All other fields are not supported by the platform.
#[derive(Clone, Copy, Debug)]
pub struct Message {
    handle: Handle,
    ecn: ExplicitCongestionNotification,
    payload_ptr: *mut u8,
    payload_len: usize,
}

impl Message {
    #[inline]
    pub(crate) fn handle(&self) -> &Handle {
        &self.handle
    }

    #[inline]
    pub(crate) fn handle_mut(&mut self) -> &mut Handle {
        &mut self.handle
    }

    #[inline]
    pub(crate) fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    #[inline]
    pub(crate) fn ecn_mut(&mut self) -> &mut ExplicitCongestionNotification {
        &mut self.ecn
    }

    #[inline]
    pub(crate) fn payload(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.payload_ptr, self.payload_len) }
    }
}

pub type Handle = path::Tuple;

impl MessageTrait for Message {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = false;
    const SUPPORTS_ECN: bool = true;
    const SUPPORTS_FLOW_LABELS: bool = false;

    #[inline]
    fn alloc(entries: u32, payload_len: u32, offset: usize) -> message::Storage {
        unsafe { alloc(entries, payload_len, offset) }
    }

    #[inline]
    fn payload_len(&self) -> usize {
        self.payload_len
    }

    #[inline]
    unsafe fn set_payload_len(&mut self, len: usize) {
        self.payload_len = len;
    }

    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        self.set_payload_len(mtu)
    }

    #[inline]
    fn payload_ptr_mut(&mut self) -> *mut u8 {
        self.payload_ptr
    }

    #[inline]
    fn validate_replication(source: &Self, dest: &Self) {
        assert_eq!(source.payload_ptr, dest.payload_ptr);
    }

    #[inline]
    fn rx_read(
        &mut self,
        _local_address: &path::LocalAddress,
    ) -> Option<message::RxMessage<'_, Self::Handle>> {
        let path = self.handle;
        let header = datagram::Header {
            path,
            ecn: self.ecn,
        };
        let payload = self.payload_mut();

        let message = message::RxMessage {
            header,
            segment_size: payload.len(),
            payload,
        };

        Some(message)
    }

    #[inline]
    fn tx_write<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<usize, tx::Error> {
        let payload = self.payload_mut();

        let len = message.write_payload(tx::PayloadBuffer::new(payload), 0)?;

        unsafe {
            debug_assert!(len <= payload.len());
            let len = len.min(payload.len());
            self.set_payload_len(len);
        }

        self.handle = *message.path_handle();
        self.ecn = message.ecn();

        Ok(len)
    }
}

#[inline]
unsafe fn alloc(entries: u32, payload_len: u32, offset: usize) -> message::Storage {
    let (layout, entry_offset, payload_offset) = layout(entries, payload_len, offset);

    let storage = message::Storage::new(layout);

    {
        let ptr = storage.as_ptr();

        let mut entry_ptr = ptr.add(entry_offset) as *mut Message;
        let mut payload_ptr = ptr.add(payload_offset);
        for _ in 0..entries {
            let entry = &mut *entry_ptr;
            entry.payload_ptr = payload_ptr;
            entry.payload_len = payload_len as _;

            entry_ptr = entry_ptr.add(1);
            payload_ptr = payload_ptr.add(payload_len as _);
            storage.check_bounds(entry_ptr);
            storage.check_bounds(payload_ptr);
        }

        let primary = ptr.add(entry_offset) as *mut Message;
        let secondary = primary.add(entries as _);
        storage.check_bounds(secondary.add(entries as _));
        core::ptr::copy_nonoverlapping(primary, secondary, entries as _);
    }

    storage
}

fn layout(entries: u32, payload_len: u32, offset: usize) -> (Layout, usize, usize) {
    let cursor = Layout::array::<u8>(offset).unwrap();
    let payloads = Layout::array::<u8>(entries as usize * payload_len as usize).unwrap();
    let entries = Layout::array::<Message>((entries * 2) as usize).unwrap();
    let (layout, entry_offset) = cursor.extend(entries).unwrap();
    let (layout, payload_offset) = layout.extend(payloads).unwrap();
    (layout, entry_offset, payload_offset)
}

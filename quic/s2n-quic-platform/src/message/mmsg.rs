// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::{msg, Message as MessageTrait};
use libc::mmsghdr;
use s2n_quic_core::{io::tx, path};

pub use libc::mmsghdr as Message;
pub type Handle = msg::Handle;

impl MessageTrait for mmsghdr {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = libc::msghdr::SUPPORTS_GSO;
    const SUPPORTS_ECN: bool = libc::msghdr::SUPPORTS_ECN;
    const SUPPORTS_FLOW_LABELS: bool = libc::msghdr::SUPPORTS_FLOW_LABELS;

    #[inline]
    fn alloc(entries: u32, payload_len: u32, offset: usize) -> super::Storage {
        unsafe {
            msg::alloc(entries, payload_len, offset, |mmsghdr: &mut mmsghdr| {
                mmsghdr.msg_len = payload_len as _;
                &mut mmsghdr.msg_hdr
            })
        }
    }

    #[inline]
    fn payload_len(&self) -> usize {
        let payload_len = self.msg_len as usize;
        debug_assert!(payload_len <= u16::MAX as usize);
        payload_len
    }

    #[inline]
    unsafe fn set_payload_len(&mut self, len: usize) {
        debug_assert!(len <= u16::MAX as usize);
        self.msg_len = len as _;
        self.msg_hdr.set_payload_len(len);
    }

    #[inline]
    fn set_segment_size(&mut self, size: usize) {
        self.msg_hdr.set_segment_size(size)
    }

    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        self.set_payload_len(mtu);
        self.msg_hdr.reset(mtu)
    }

    #[inline]
    fn payload_ptr_mut(&mut self) -> *mut u8 {
        self.msg_hdr.payload_ptr_mut()
    }

    #[inline]
    fn validate_replication(source: &Self, dest: &Self) {
        libc::msghdr::validate_replication(&source.msg_hdr, &dest.msg_hdr)
    }

    #[inline]
    fn rx_read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<super::RxMessage<'_, Self::Handle>> {
        unsafe {
            // We need to replicate the `msg_len` field to the inner type before delegating
            // Safety: The `msg_len` is associated with the same buffer as the `msg_hdr`
            self.msg_hdr.set_payload_len(self.msg_len as _);
        }
        self.msg_hdr.rx_read(local_address)
    }

    #[inline]
    fn tx_write<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        message: M,
    ) -> Result<usize, tx::Error> {
        let len = self.msg_hdr.tx_write(message)?;
        // We need to replicate the len with the `msg_len` field after delegating to `msg_hdr`
        debug_assert!(len <= u16::MAX as usize);
        self.msg_len = len as _;
        Ok(len)
    }
}

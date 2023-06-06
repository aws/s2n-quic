// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::{
    msg::{self, Ring as MsgRing},
    Message as MessageTrait,
};
use alloc::vec::Vec;
use core::mem::zeroed;
use libc::mmsghdr;
use s2n_quic_core::{io::tx, path};

pub use libc::mmsghdr as Message;
pub type Handle = msg::Handle;

impl MessageTrait for mmsghdr {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = libc::msghdr::SUPPORTS_GSO;

    #[inline]
    fn payload_len(&self) -> usize {
        self.msg_len as usize
    }

    #[inline]
    unsafe fn set_payload_len(&mut self, len: usize) {
        debug_assert!(len <= core::u32::MAX as usize);
        self.msg_len = len as _;
        self.msg_hdr.set_payload_len(len);
    }

    #[inline]
    fn can_gso<M: tx::Message<Handle = Self::Handle>>(&self, other: &mut M) -> bool {
        self.msg_hdr.can_gso(other)
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
    fn replicate_fields_from(&mut self, other: &Self) {
        self.msg_len = other.msg_len;
        self.msg_hdr.replicate_fields_from(&other.msg_hdr)
    }

    #[inline]
    fn rx_read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<super::RxMessage<Self::Handle>> {
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
        self.msg_len = len as _;
        Ok(len)
    }
}

pub struct Ring<Payloads> {
    messages: Vec<Message>,
    storage: msg::Storage<Payloads>,
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
#[allow(unknown_lints, clippy::non_send_fields_in_send_ty)]
unsafe impl<Payloads: Send> Send for Ring<Payloads> {}

impl<Payloads: crate::buffer::Buffer + Default> Default for Ring<Payloads> {
    fn default() -> Self {
        Self::new(
            Payloads::default(),
            crate::features::gso::MaxSegments::DEFAULT.into(),
        )
    }
}

impl<Payloads: crate::buffer::Buffer> Ring<Payloads> {
    pub fn new(payloads: Payloads, max_gso: usize) -> Self {
        let MsgRing {
            mut messages,
            storage,
        } = MsgRing::new(payloads, max_gso);

        // convert msghdr into mmsghdr
        let messages = messages
            .drain(..)
            .map(|msg_hdr| unsafe {
                let mut mmsghdr = zeroed::<mmsghdr>();
                let payload_len = msg_hdr.payload_len();
                mmsghdr.msg_hdr = msg_hdr;
                mmsghdr.set_payload_len(payload_len);
                mmsghdr
            })
            .collect();

        Self { messages, storage }
    }
}

impl<Payloads: crate::buffer::Buffer> super::Ring for Ring<Payloads> {
    type Message = Message;

    #[inline]
    fn len(&self) -> usize {
        self.messages.len() / 2
    }

    #[inline]
    fn mtu(&self) -> usize {
        self.storage.mtu()
    }

    #[inline]
    fn max_gso(&self) -> usize {
        self.storage.max_gso()
    }

    #[inline]
    fn disable_gso(&mut self) {
        // TODO recompute message offsets
        // https://github.com/aws/s2n-quic/issues/762
        self.storage.disable_gso()
    }

    #[inline]
    fn as_slice(&self) -> &[Self::Message] {
        &self.messages[..]
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [Self::Message] {
        &mut self.messages[..]
    }
}

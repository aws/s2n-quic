// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(unused_macros)]

macro_rules! impl_message_delegate {
    ($name:ident, $field:tt, $field_ty:ty) => {
        impl $crate::message::Message for $name {
            const SUPPORTS_GSO: bool = <$field_ty as $crate::message::Message>::SUPPORTS_GSO;

            fn ecn(&self) -> ExplicitCongestionNotification {
                $crate::message::Message::ecn(&self.$field)
            }

            fn set_ecn(
                &mut self,
                ecn: ExplicitCongestionNotification,
                remote_address: &SocketAddress,
            ) {
                $crate::message::Message::set_ecn(&mut self.$field, ecn, remote_address)
            }

            fn remote_address(&self) -> Option<SocketAddress> {
                $crate::message::Message::remote_address(&self.$field)
            }

            fn set_remote_address(&mut self, remote_address: &SocketAddress) {
                $crate::message::Message::set_remote_address(&mut self.$field, remote_address)
            }

            fn payload_len(&self) -> usize {
                $crate::message::Message::payload_len(&self.$field)
            }

            unsafe fn set_payload_len(&mut self, payload_len: usize) {
                $crate::message::Message::set_payload_len(&mut self.$field, payload_len)
            }

            fn set_segment_size(&mut self, size: usize) {
                $crate::message::Message::set_segment_size(&mut self.$field, size)
            }

            unsafe fn reset(&mut self, mtu: usize) {
                $crate::message::Message::reset(&mut self.$field, mtu)
            }

            fn replicate_fields_from(&mut self, other: &Self) {
                $crate::message::Message::replicate_fields_from(&mut self.$field, &other.$field)
            }

            fn payload_ptr(&self) -> *const u8 {
                $crate::message::Message::payload_ptr(&self.$field)
            }

            fn payload_ptr_mut(&mut self) -> *mut u8 {
                $crate::message::Message::payload_ptr_mut(&mut self.$field)
            }
        }
    };
}

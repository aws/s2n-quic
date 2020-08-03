#![allow(unused_macros)]

macro_rules! impl_message_delegate {
    ($name:ident, $field:tt) => {
        impl $crate::message::Message for $name {
            fn ecn(&self) -> ExplicitCongestionNotification {
                $crate::message::Message::ecn(&self.$field)
            }

            fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
                $crate::message::Message::set_ecn(&mut self.$field, ecn)
            }

            fn remote_address(&self) -> Option<SocketAddress> {
                $crate::message::Message::remote_address(&self.$field)
            }

            fn set_remote_address(&mut self, remote_address: &SocketAddress) {
                $crate::message::Message::set_remote_address(&mut self.$field, remote_address)
            }

            fn reset_remote_address(&mut self) {
                $crate::message::Message::reset_remote_address(&mut self.$field)
            }

            fn payload_len(&self) -> usize {
                $crate::message::Message::payload_len(&self.$field)
            }

            unsafe fn set_payload_len(&mut self, payload_len: usize) {
                $crate::message::Message::set_payload_len(&mut self.$field, payload_len)
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

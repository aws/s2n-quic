// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::Buffer, value::Parsed};
use std::{ffi::CStr, time::Duration};

pub(crate) trait Info {
    fn is_empty(&self) -> bool;
    fn append<T: core::fmt::Display>(&mut self, v: T);
    fn append_str(&mut self, v: &str);
    fn append_delim(&mut self, v: &str) {
        if !self.is_empty() {
            self.append_str(v);
        }
    }
}

impl Info for () {
    fn is_empty(&self) -> bool {
        true
    }

    fn append<T: core::fmt::Display>(&mut self, v: T) {
        let _ = v;
    }

    fn append_str(&mut self, v: &str) {
        let _ = v;
    }
}

impl Info for Vec<u8> {
    fn is_empty(&self) -> bool {
        (*self).is_empty()
    }

    fn append<T: core::fmt::Display>(&mut self, v: T) {
        use std::io::Write;
        let _ = write!(self, "{}", v);
    }

    fn append_str(&mut self, v: &str) {
        self.extend_from_slice(v.as_bytes());
    }
}

impl Info for String {
    fn is_empty(&self) -> bool {
        (*self).is_empty()
    }

    fn append<T: core::fmt::Display>(&mut self, v: T) {
        use std::fmt::Write;
        let _ = write!(self, "{}", v);
    }

    fn append_str(&mut self, v: &str) {
        *self += v;
    }
}

pub(crate) trait Item {
    fn append_text(&mut self, text: &'static CStr);
}

pub(crate) trait Node {
    type AddedItem: Item;
    fn add_slice(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<&[u8]>) -> Self::AddedItem;
    fn add_slice_hidden(
        &mut self,
        buffer: &Buffer,
        field: i32,
        parsed: Parsed<&[u8]>,
    ) -> Self::AddedItem;
    fn add_u64(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u64>) -> Self::AddedItem;
    fn add_u32(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u32>) -> Self::AddedItem;
    fn add_u16(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u16>) -> Self::AddedItem;
    fn add_u8(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u8>) -> Self::AddedItem;
    // This takes a `u8` because it's typically used with a bitmask pointing at a specific bit.
    fn add_boolean<T: Into<u8>>(
        &mut self,
        buffer: &Buffer,
        field: i32,
        parsed: Parsed<T>,
    ) -> Self::AddedItem;
    fn add_duration(
        &mut self,
        buffer: &Buffer,
        field: i32,
        parsed: Parsed<Duration>,
    ) -> Self::AddedItem;

    fn add_subtree(&mut self, item: Self::AddedItem, id: i32) -> Self;
}

#[cfg(not(test))]
mod wireshark_sys_impl {
    use super::*;
    use crate::wireshark_sys;

    impl Item for *mut wireshark_sys::proto_item {
        fn append_text(&mut self, text: &'static CStr) {
            unsafe {
                wireshark_sys::proto_item_append_text(*self, c"%s".as_ptr(), text.as_ptr());
            }
        }
    }

    impl Item for *mut wireshark_sys::_packet_info {
        fn append_text(&mut self, text: &CStr) {
            unsafe {
                wireshark_sys::col_append_str(
                    (**self).cinfo,
                    wireshark_sys::COL_INFO as _,
                    text.as_ptr(),
                );
            }
        }
    }

    impl Node for *mut wireshark_sys::_proto_node {
        type AddedItem = *mut wireshark_sys::proto_item;

        fn add_u64(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u64>) -> Self::AddedItem {
            unsafe {
                wireshark_sys::proto_tree_add_uint64(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value,
                )
            }
        }

        fn add_u32(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u32>) -> Self::AddedItem {
            unsafe {
                wireshark_sys::proto_tree_add_uint(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value,
                )
            }
        }

        fn add_u16(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u16>) -> Self::AddedItem {
            unsafe {
                wireshark_sys::proto_tree_add_uint(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value as u32,
                )
            }
        }

        fn add_u8(&mut self, buffer: &Buffer, field: i32, parsed: Parsed<u8>) -> Self::AddedItem {
            unsafe {
                wireshark_sys::proto_tree_add_uint(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value as u32,
                )
            }
        }

        fn add_boolean<T: Into<u8>>(
            &mut self,
            buffer: &Buffer,
            field: i32,
            parsed: Parsed<T>,
        ) -> Self::AddedItem {
            unsafe {
                wireshark_sys::proto_tree_add_boolean(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value.into() as u64,
                )
            }
        }

        fn add_slice(
            &mut self,
            buffer: &Buffer,
            field: i32,
            parsed: Parsed<&[u8]>,
        ) -> Self::AddedItem {
            unsafe {
                wireshark_sys::proto_tree_add_bytes_with_length(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value.as_ptr(),
                    parsed.value.len() as _,
                )
            }
        }

        fn add_slice_hidden(
            &mut self,
            buffer: &Buffer,
            field: i32,
            parsed: Parsed<&[u8]>,
        ) -> Self::AddedItem {
            let fmt = format!("({} bytes)\0", parsed.len);
            unsafe {
                wireshark_sys::proto_tree_add_bytes_format_value(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    parsed.value.as_ptr(),
                    fmt.as_ptr() as *const _,
                )
            }
        }

        fn add_duration(
            &mut self,
            buffer: &Buffer,
            field: i32,
            parsed: Parsed<Duration>,
        ) -> Self::AddedItem {
            let time = wireshark_sys::nstime_t {
                secs: parsed.value.as_secs() as i64,
                nsecs: parsed.value.subsec_nanos() as i32,
            };
            unsafe {
                wireshark_sys::proto_tree_add_time(
                    *self,
                    field,
                    buffer.tvb,
                    parsed.offset as _,
                    parsed.len as _,
                    &time,
                )
            }
        }

        fn add_subtree(&mut self, item: Self::AddedItem, id: i32) -> Self {
            unsafe { wireshark_sys::proto_item_add_subtree(item, id) }
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Aya does not currently support XSK maps
//!
//! TODO replace with the official one once https://github.com/aya-rs/aya/pull/527 is merged

use aya_bpf::{
    bindings::{bpf_map_def, bpf_map_type::BPF_MAP_TYPE_XSKMAP},
    helpers::bpf_redirect_map,
};
use core::{cell::UnsafeCell, mem};

#[repr(transparent)]
pub struct XskMap {
    def: UnsafeCell<bpf_map_def>,
}

unsafe impl Sync for XskMap {}

impl XskMap {
    pub const fn with_max_entries(max_entries: u32, flags: u32) -> Self {
        Self {
            def: UnsafeCell::new(bpf_map_def {
                type_: BPF_MAP_TYPE_XSKMAP,
                key_size: mem::size_of::<u32>() as u32,
                value_size: mem::size_of::<u32>() as u32,
                max_entries,
                map_flags: flags,
                id: 0,
                // pinning: PinningType::None as u32,
                pinning: 0 as _,
            }),
        }
    }

    #[inline(always)]
    pub fn redirect(&self, index: u32, flags: u64) -> u32 {
        unsafe { bpf_redirect_map(self.def.get() as *mut _, index as _, flags) as u32 }
    }
}

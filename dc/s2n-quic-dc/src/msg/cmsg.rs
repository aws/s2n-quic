// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use libc::msghdr;
use s2n_quic_core::{ensure, inet::ExplicitCongestionNotification};
use s2n_quic_platform::{features, message::cmsg};

pub use cmsg::*;

pub const ENCODER_LEN: usize = {
    // TODO calculate based on platform support
    128
};

pub const DECODER_LEN: usize = {
    // TODO calculate based on platform support
    128
};

pub const MAX_GRO_SEGMENTS: usize = features::gro::MAX_SEGMENTS;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Receiver {
    ecn: ExplicitCongestionNotification,
    segment_len: u16,
}

impl Receiver {
    #[inline]
    pub fn with_msg(&mut self, msg: &msghdr) {
        // assume we didn't get a GRO cmsg initially
        self.segment_len = 0;

        ensure!(!msg.msg_control.is_null());
        ensure!(msg.msg_controllen > 0);

        let iter = unsafe {
            // SAFETY: the msghdr controllen should be aligned
            cmsg::decode::Iter::from_msghdr(msg)
        };

        for (cmsg, value) in iter {
            match (cmsg.cmsg_level, cmsg.cmsg_type) {
                (level, ty) if features::tos::is_match(level, ty) => {
                    if let Some(ecn) = features::tos::decode(value) {
                        // TODO remove this conversion once we consolidate the s2n-quic-core crates
                        // convert between the vendored s2n-quic-core types
                        let ecn = {
                            let ecn = ecn as u8;
                            ExplicitCongestionNotification::new(ecn)
                        };
                        self.ecn = ecn;
                    } else {
                        continue;
                    }
                }
                (level, ty) if features::gso::is_match(level, ty) => {
                    // ignore GSO settings when reading
                    continue;
                }
                (level, ty) if features::gro::is_match(level, ty) => {
                    if let Some(segment_size) =
                        unsafe { cmsg::decode::value_from_bytes::<features::gro::Cmsg>(value) }
                    {
                        self.segment_len = segment_size as _;
                    } else {
                        continue;
                    }
                }
                _ => {
                    continue;
                }
            }
        }
    }

    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        self.ecn
    }

    #[inline]
    pub fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
        self.ecn = ecn;
    }

    #[inline]
    pub fn segment_len(&self) -> u16 {
        self.segment_len
    }

    #[inline]
    pub fn set_segment_len(&mut self, len: u16) {
        self.segment_len = len;
    }

    #[inline]
    pub fn take_segment_len(&mut self) -> u16 {
        core::mem::replace(&mut self.segment_len, 0)
    }
}

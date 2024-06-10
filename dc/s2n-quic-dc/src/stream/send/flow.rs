// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;

pub mod blocking;
pub mod non_blocking;

/// The maximum payload allowed in sendmsg calls
///
/// > https://github.com/torvalds/linux/blob/8cd26fd90c1ad7acdcfb9f69ca99d13aa7b24561/net/ipv4/ip_output.c#L987-L995
/// > Linux enforces a u16::MAX - IP_HEADER_LEN - UDP_HEADER_LEN
pub const MAX_PAYLOAD: u16 = u16::MAX - 50;

/// Flow credits acquired by an application request
#[derive(Debug)]
pub struct Credits {
    /// The offset at which to write the stream bytes
    pub offset: VarInt,
    /// The number of bytes which an application must write after acquisition
    pub len: usize,
    /// The total number of bytes the application buffer is willing to transmit
    pub initial_len: usize,
    /// Indicates if the stream is being finalized
    pub is_fin: bool,
}

/// An application request for flow credits
#[derive(Clone, Copy, Debug)]
pub struct Request {
    /// The number of bytes in the application buffer
    pub len: usize,
    /// The total number of bytes the application buffer is willing to transmit
    pub initial_len: usize,
    /// Indicates if the request is finalizing a stream
    pub is_fin: bool,
}

impl Request {
    /// Clamps the request with the given number of credits
    #[inline]
    pub fn clamp(&mut self, credits: u64) {
        let len = self.len.min(credits.min(MAX_PAYLOAD as _) as usize);

        // if we didn't acquire the entire len, then clear the `is_fin` flag
        if self.len != len {
            self.is_fin = false;
        }

        // update the len based on the provided credits
        self.len = len;
    }

    /// Constructs a response with the acquired offset
    #[inline]
    pub fn response(self, offset: VarInt) -> Credits {
        Credits {
            offset,
            len: self.len,
            initial_len: self.initial_len,
            is_fin: self.is_fin,
        }
    }
}

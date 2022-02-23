// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::Poll;

#[derive(Clone, Debug, Default)]
pub struct IdPrefixReader {
    bytes: [u8; core::mem::size_of::<u64>()],
    cursor: u8,
}

impl IdPrefixReader {
    pub fn remaining(&mut self) -> &mut [u8] {
        &mut self.bytes[self.cursor as usize..]
    }

    pub fn on_read(&mut self, len: usize) -> Poll<u64> {
        let cursor = self.cursor as usize + len;
        if cursor >= self.bytes.len() {
            Poll::Ready(u64::from_be_bytes(self.bytes))
        } else {
            self.cursor = cursor as u8;
            Poll::Pending
        }
    }
}

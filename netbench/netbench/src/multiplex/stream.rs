// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Blocked;
use crate::Result;
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_quic_core::stream::testing::Data;

#[derive(Debug)]
pub struct Stream {
    pub rx: Option<ReceiveStream>,
    pub tx: Option<SendStream>,
}

#[derive(Debug)]
pub struct ReceiveStream {
    received_offset: u64,
    buffered_offset: u64,
    window_offset: u64,
    is_finished: bool,
    blocked: Blocked,
}

impl ReceiveStream {
    pub fn new(window_offset: u64) -> Self {
        Self {
            received_offset: 0,
            buffered_offset: 0,
            window_offset,
            is_finished: false,
            blocked: Default::default(),
        }
    }

    pub fn buffer(&mut self, len: u64) -> Result<u64> {
        if self.is_finished {
            return Err("stream is already finished".into());
        }

        let len = (self.window_offset - self.buffered_offset).min(len);

        if len == 0 {
            return Ok(0);
        }

        self.buffered_offset += len;

        self.blocked.unblock();

        Ok(len)
    }

    pub fn receive(&mut self, len: u64, cx: &mut Context) -> Poll<Result<u64>> {
        let len = (self.buffered_offset - self.received_offset).min(len);

        if len == 0 {
            return if self.is_finished {
                Ok(0).into()
            } else {
                self.blocked.block(cx);
                Poll::Pending
            };
        }

        self.received_offset += len;

        Ok(len).into()
    }

    pub fn finish(&mut self) {
        self.is_finished = true;
        self.blocked.unblock();
    }

    pub fn receive_window(&self) -> u64 {
        self.window_offset - self.received_offset
    }

    pub fn credit(&mut self, credits: u64) -> u64 {
        self.window_offset += credits;
        self.window_offset
    }

    pub fn poll_finish(&mut self, cx: &mut Context) -> Poll<()> {
        if self.is_finished {
            Poll::Ready(())
        } else {
            self.blocked.block(cx);
            Poll::Pending
        }
    }
}

#[derive(Debug, Default)]
pub struct SendStream {
    max_data: u64,
    data: Data,
    blocked: Blocked,
}

impl SendStream {
    pub fn new(max_data: u64) -> Self {
        Self {
            max_data,
            data: Data::new(u64::MAX),
            blocked: Default::default(),
        }
    }

    pub fn max_data(&mut self, max_data: u64) {
        self.max_data = max_data.max(self.max_data);
        self.blocked.unblock()
    }

    pub fn send(&mut self, len: u64, cx: &mut Context) -> Option<Bytes> {
        let window = self.max_data - self.data.offset();
        let len = len.min(window) as usize;

        if len == 0 {
            self.blocked.block(cx);
            return None;
        }

        self.data.send_one(len)
    }
}

#[derive(Debug, Default)]
pub struct Controller {
    open_offset: u64,
    peer_window_offset: u64,
    local_window_offset: u64,
    transmitted_window_offset: u64,
    blocked: Blocked,
}

impl Controller {
    pub fn new(local_window: u64, peer_window: u64) -> Self {
        let local_window = local_window.max(1);
        let peer_window = peer_window.max(1);
        Self {
            open_offset: 0,
            peer_window_offset: local_window,
            local_window_offset: peer_window,
            transmitted_window_offset: local_window.min(peer_window),
            blocked: Default::default(),
        }
    }

    pub fn open(&mut self, cx: &mut Context) -> Poll<()> {
        if self.capacity() == 0 {
            self.blocked.block(cx);
            return Poll::Pending;
        }

        self.open_offset += 1;

        Poll::Ready(())
    }

    pub fn close(&mut self) {
        self.local_window_offset += 1;
        if self.capacity() > 0 {
            self.blocked.unblock();
        }
    }

    pub fn transmit(&mut self) -> Option<u64> {
        // send a max streams if we are using more than half capacity
        if self.local_window_offset >= self.transmitted_window_offset * 2 {
            self.transmitted_window_offset = self.local_window_offset;
            Some(self.local_window_offset)
        } else {
            None
        }
    }

    pub fn max_streams(&mut self, up_to: u64) {
        self.peer_window_offset = up_to;
        if self.capacity() > 0 {
            self.blocked.unblock()
        }
    }

    pub fn capacity(&self) -> u64 {
        self.local_window_offset.min(self.peer_window_offset) - self.open_offset
    }
}

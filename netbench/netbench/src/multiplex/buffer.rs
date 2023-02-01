// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Frame;
use crate::Result;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use core::{
    mem::MaybeUninit,
    task::{Context, Poll},
};
use futures::ready;
use std::{collections::VecDeque, io::IoSlice};
use tokio::io::ReadBuf;

const CHUNK_CAPACITY: usize = 4096;

#[derive(Debug, Default)]
pub struct ReadBuffer {
    len: usize,
    tail: BytesMut,
    head: VecDeque<BytesMut>,
}

impl ReadBuffer {
    pub fn read<F: FnOnce(&mut ReadBuf) -> Poll<Result<()>>>(
        &mut self,
        read: F,
    ) -> Poll<Result<()>> {
        let capacity = self.tail.capacity();

        if capacity == 0 {
            self.tail.reserve(CHUNK_CAPACITY);
        } else if capacity < 32 {
            let tail = core::mem::replace(&mut self.tail, BytesMut::with_capacity(CHUNK_CAPACITY));
            self.head.push_back(tail);
        }

        let buf = self.tail.chunk_mut();
        let buf = unsafe { &mut *(buf as *mut _ as *mut [MaybeUninit<u8>]) };
        let mut buf = ReadBuf::uninit(buf);
        ready!(read(&mut buf))?;
        let len = buf.filled().len();

        if len == 0 {
            return Ok(()).into();
        }

        unsafe {
            self.tail.advance_mut(len);
            self.len += len;
        }

        Ok(()).into()
    }

    fn check_consistency(&self) {
        if cfg!(debug_assertions) {
            let mut actual_len = self.tail.len();
            for chunk in self.head.iter() {
                actual_len += chunk.len();
            }
            assert_eq!(actual_len, self.remaining(), "{self:?}");
        }
    }
}

impl Buf for ReadBuffer {
    fn remaining(&self) -> usize {
        self.len
    }

    fn chunk(&self) -> &[u8] {
        for chunk in self.head.iter() {
            if !chunk.is_empty() {
                return chunk.chunk();
            }
        }

        self.tail.chunk()
    }

    fn advance(&mut self, mut cnt: usize) {
        while cnt > 0 {
            let len = if let Some(front) = self.head.front_mut() {
                let len = front.len().min(cnt);
                front.advance(len);
                if front.is_empty() {
                    let _ = self.head.pop_front();
                }
                len
            } else {
                self.tail.advance(cnt);
                cnt
            };

            cnt -= len;
            self.len -= len;
        }

        self.check_consistency();
    }

    fn copy_to_bytes(&mut self, len: usize) -> Bytes {
        while let Some(mut front) = self.head.pop_front() {
            if front.is_empty() {
                continue;
            }

            self.len -= len;

            if front.len() == len {
                self.check_consistency();
                return front.freeze();
            }

            let out = front.split_to(len);
            self.head.push_front(front);

            self.check_consistency();
            return out.freeze();
        }

        let out = self.tail.split_to(len).freeze();

        self.len -= len;
        self.check_consistency();

        out
    }
}

#[derive(Debug, Default)]
pub struct WriteBuffer {
    unfilled: BytesMut,
    queue: VecDeque<Bytes>,
    push_interest: bool,
}

impl WriteBuffer {
    pub fn request_push(&mut self, max_queue_len: usize) -> bool {
        let can_push = max_queue_len > self.queue.len();
        self.push_interest |= !can_push;
        can_push
    }

    pub fn push(&mut self, frame: Frame) {
        let capacity = self.unfilled.capacity();

        if capacity == 0 {
            self.unfilled.reserve(CHUNK_CAPACITY);
        } else if capacity < 32 {
            self.unfilled = BytesMut::with_capacity(CHUNK_CAPACITY);
        }

        frame.write_header(&mut self.unfilled);
        self.queue.push_back(self.unfilled.split().freeze());

        if let Some(data) = frame.body() {
            self.queue.push_back(data);
        }
    }

    pub fn push_priority(&mut self, frame: Frame) {
        let capacity = self.unfilled.capacity();

        const CAPACITY: usize = 4096;

        if capacity == 0 {
            self.unfilled.reserve(CAPACITY);
        } else if capacity < 32 {
            self.unfilled = BytesMut::with_capacity(CAPACITY);
        }

        frame.write_header(&mut self.unfilled);
        let header = self.unfilled.split().freeze();

        // push the body first
        if let Some(data) = frame.body() {
            self.queue.push_front(data);
        }

        self.queue.push_front(header);
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn pop_front(&mut self) -> Option<Bytes> {
        self.queue.pop_front()
    }

    pub fn push_front(&mut self, chunk: Bytes) {
        self.queue.push_front(chunk);
    }

    pub fn notify(&mut self, cx: &mut Context) {
        if self.push_interest {
            self.push_interest = false;
            cx.waker().wake_by_ref();
        }
    }

    pub fn advance(&mut self, mut len: usize) {
        while let Some(mut chunk) = self.queue.pop_front() {
            let next_len = len.saturating_sub(chunk.len());

            if next_len > 0 {
                len = next_len;
                continue;
            }

            if chunk.len() > len {
                chunk.advance(len);
                self.queue.push_front(chunk);
            }

            return;
        }
    }

    pub fn chunks(&self) -> Vec<IoSlice> {
        self.queue.iter().map(|v| IoSlice::new(v)).collect()
    }
}

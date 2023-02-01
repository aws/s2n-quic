// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{state::Side, Cursor, Result, State};
use core::task::{Context, Poll};

pub struct Receiver<T>(pub(super) State<T>);

impl<T> Receiver<T> {
    #[inline]
    pub fn capacity(&self) -> usize {
        self.0.cursor.capacity()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.cursor.recv_len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.cursor.is_empty()
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.0.cursor.is_full()
    }

    #[inline]
    pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<RecvSlice<T>>> {
        macro_rules! acquire_filled {
            () => {
                match self.0.acquire_filled() {
                    Ok(true) => {
                        let cursor = self.0.cursor;
                        return Ok(RecvSlice(&mut self.0, cursor)).into();
                    }
                    Ok(false) => {
                        // the queue is full
                    }
                    Err(err) => {
                        // the channel was closed
                        return Err(err).into();
                    }
                }
            };
        }

        // check capacity before registering a waker
        acquire_filled!();

        // register the waker
        self.0.receiver.register(cx.waker());

        // check once more to avoid a loss of notification
        acquire_filled!();

        Poll::Pending
    }

    #[inline]
    pub fn try_slice(&mut self) -> Result<Option<RecvSlice<T>>> {
        Ok(if self.0.acquire_filled()? {
            let cursor = self.0.cursor;
            Some(RecvSlice(&mut self.0, cursor))
        } else {
            None
        })
    }
}

impl<T> Drop for Receiver<T> {
    #[inline]
    fn drop(&mut self) {
        self.0.try_close(Side::Receiver);
    }
}

pub struct RecvSlice<'a, T>(&'a mut State<T>, Cursor);

impl<'a, T> RecvSlice<'a, T> {
    #[inline]
    pub fn peek(&mut self) -> (&mut [T], &mut [T]) {
        let _ = self.0.acquire_filled();
        let (slice, _) = self.0.as_pairs();
        unsafe { slice.assume_init().into_mut() }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.0.cursor.is_empty() && !self.0.acquire_filled().unwrap_or(false) {
            return None;
        }

        let (pair, _) = self.0.as_pairs();
        let value = unsafe { pair.take(0) };
        self.0.cursor.increment_head(1);
        Some(value)
    }

    #[inline]
    pub fn clear(&mut self) -> usize {
        // don't update the cursor so the caller can observe any updates through peek

        let (pair, _) = self.0.as_pairs();
        let len = pair.len();

        for entry in pair.iter() {
            unsafe {
                let _ = entry.take();
            }
        }

        self.0.cursor.increment_head(len);

        len
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.cursor.recv_len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.cursor.is_empty()
    }
}

impl<'a, T> Drop for RecvSlice<'a, T> {
    #[inline]
    fn drop(&mut self) {
        self.0.persist_head(self.1);
    }
}

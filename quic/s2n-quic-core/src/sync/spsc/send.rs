// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{state::Side, Cursor, PushError, Result, State};
use core::task::{Context, Poll};

pub struct Sender<T>(pub(super) State<T>);

impl<T> Sender<T> {
    #[inline]
    pub fn capacity(&self) -> usize {
        self.0.cursor.capacity()
    }

    #[inline]
    pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<SendSlice<T>>> {
        macro_rules! acquire_capacity {
            () => {
                match self.0.acquire_capacity() {
                    Ok(true) => {
                        let cursor = self.0.cursor;
                        return Ok(SendSlice(&mut self.0, cursor)).into();
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
        acquire_capacity!();

        // register the waker
        self.0.sender.register(cx.waker());

        // check once more to avoid a loss of notification
        acquire_capacity!();

        Poll::Pending
    }

    #[inline]
    pub fn try_slice(&mut self) -> Result<Option<SendSlice<T>>> {
        Ok(if self.0.acquire_capacity()? {
            let cursor = self.0.cursor;
            Some(SendSlice(&mut self.0, cursor))
        } else {
            None
        })
    }
}

impl<T> Drop for Sender<T> {
    #[inline]
    fn drop(&mut self) {
        self.0.close(Side::Sender);
    }
}

pub struct SendSlice<'a, T>(&'a mut State<T>, Cursor);

impl<'a, T> SendSlice<'a, T> {
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), PushError<T>> {
        if self.0.cursor.is_full() && !self.0.acquire_capacity()? {
            return Err(PushError::Full(value));
        }

        let (_, pair) = self.0.as_pairs();

        unsafe {
            // Safety: the second pair of slices contains uninitialized memory and the cursor
            // indicates we have capacity to write at least one value
            pair.write(0, value);
        }

        self.0.cursor.increment_tail(1);

        Ok(())
    }

    pub fn extend<I: Iterator<Item = T>>(&mut self, iter: &mut I) -> Result<()> {
        if self.0.acquire_capacity()? {
            let (_, pair) = self.0.as_pairs();

            let mut idx = 0;
            let capacity = self.capacity();

            while idx < capacity {
                if let Some(value) = iter.next() {
                    unsafe {
                        // Safety: the second pair of slices contains uninitialized memory
                        pair.write(idx, value);
                    }
                    idx += 1;
                } else {
                    break;
                }
            }

            self.0.cursor.increment_tail(idx);
        }

        Ok(())
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.0.cursor.send_capacity()
    }
}

impl<'a, T> Drop for SendSlice<'a, T> {
    #[inline]
    fn drop(&mut self) {
        self.0.persist_tail(self.1);
    }
}

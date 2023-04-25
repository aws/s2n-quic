// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{state::Side, Cursor, Result, State};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

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

    /// Returns the currently acquired slice of entries for the receiver
    ///
    /// Callers should call [`Self::acquire`] or [`Self::poll_slice`] before calling this method.
    #[inline]
    pub fn slice(&mut self) -> RecvSlice<T> {
        let cursor = self.0.cursor;
        RecvSlice(&mut self.0, cursor)
    }

    /// Blocks until at least one entry is available for consumption
    #[inline]
    pub async fn acquire(&mut self) -> Result<()> {
        Acquire { receiver: self }.await
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
        self.0.close(Side::Receiver);
    }
}

pub struct RecvSlice<'a, T>(&'a mut State<T>, Cursor);

impl<'a, T> RecvSlice<'a, T> {
    #[inline]
    pub fn peek(&mut self) -> (&mut [T], &mut [T]) {
        let _ = self.0.acquire_filled();
        let (slice, _) = self.0.as_pairs();
        unsafe {
            // Safety: the first pair of returned slices is the `initialized` half
            slice.assume_init().into_mut()
        }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.0.cursor.is_empty() && !self.0.acquire_filled().unwrap_or(false) {
            return None;
        }

        let (pair, _) = self.0.as_pairs();
        let value = unsafe {
            // Safety: the state's cursor indicates that the first slot contains initialized data
            pair.take(0)
        };
        self.0.cursor.increment_head(1);
        Some(value)
    }

    #[inline]
    pub fn clear(&mut self) -> usize {
        // don't try to `acquire_filled` so the caller can observe any updates through peek/pop

        let (pair, _) = self.0.as_pairs();
        let len = pair.len();

        for entry in pair.iter() {
            unsafe {
                // Safety: the state's cursor indicates that each slot in the `iter` contains data
                let _ = entry.take();
            }
        }

        self.0.cursor.increment_head(len);

        len
    }

    /// Releases `len` entries back to the sender
    #[inline]
    pub fn release(&mut self, len: usize) {
        let (pair, _) = self.0.as_pairs();

        debug_assert!(pair.len() >= len, "cannot release more than was acquired");

        for entry in pair.iter().take(len) {
            unsafe {
                // Safety: the state's cursor indicates that each slot in the `iter` contains data
                let _ = entry.take();
            }
        }

        self.0.cursor.increment_head(len);
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

struct Acquire<'a, T> {
    receiver: &'a mut Receiver<T>,
}

impl<'a, T> Future for Acquire<'a, T> {
    type Output = Result<()>;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.receiver.poll_slice(cx) {
            Poll::Ready(v) => Poll::Ready(v.map(|_| ())),
            Poll::Pending => Poll::Pending,
        }
    }
}

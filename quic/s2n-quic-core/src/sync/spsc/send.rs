// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{state::Side, Cursor, PushError, Result, State};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct Sender<T>(pub(super) State<T>);

impl<T> Sender<T> {
    #[inline]
    pub fn capacity(&self) -> usize {
        self.0.cursor.capacity()
    }

    /// Returns the currently acquired slice of entries for the sender
    ///
    /// Callers should call [`Self::acquire`] or [`Self::poll_slice`] before calling this method.
    #[inline]
    pub fn slice(&mut self) -> SendSlice<'_, T> {
        let cursor = self.0.cursor;
        SendSlice(&mut self.0, cursor)
    }

    /// Blocks until at least one entry is available for sending
    #[inline]
    pub async fn acquire(&mut self) -> Result<()> {
        Acquire { sender: self }.await
    }

    #[inline]
    pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<SendSlice<'_, T>>> {
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
    pub fn try_slice(&mut self) -> Result<Option<SendSlice<'_, T>>> {
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

#[derive(Debug)]
pub struct SendSlice<'a, T>(&'a mut State<T>, Cursor);

impl<T> SendSlice<'_, T> {
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

    /// Synchronizes any updates from the receiver
    ///
    /// This can be useful for when `slice` is called without polling for entries first.
    #[inline]
    pub fn sync(&mut self) -> Result<(), super::ClosedError> {
        self.0.acquire_capacity()?;
        Ok(())
    }
}

impl<T> Drop for SendSlice<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.0.persist_tail(self.1);
    }
}

struct Acquire<'a, T> {
    sender: &'a mut Sender<T>,
}

impl<T> Future for Acquire<'_, T> {
    type Output = Result<()>;

    #[inline]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.sender.poll_slice(cx) {
            Poll::Ready(v) => Poll::Ready(v.map(|_| ())),
            Poll::Pending => Poll::Pending,
        }
    }
}

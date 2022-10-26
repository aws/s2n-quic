// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Behavior, Cursor, DoubleRing, PushError, Result, Ring, State};
use core::task::{Context, Poll};

macro_rules! impl_send {
    ($name:ident, $B:ty) => {
        pub mod $name {
            use super::*;

            pub struct Sender<T>(pub(super) super::Sender<T, $B>);

            impl<T> Sender<T> {
                #[inline]
                pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<SendSlice<T>>> {
                    match self.0.poll_slice(cx) {
                        Poll::Ready(Ok(s)) => Poll::Ready(Ok(SendSlice(s))),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                        Poll::Pending => Poll::Pending,
                    }
                }

                #[inline]
                pub fn try_slice(&mut self) -> Result<Option<SendSlice<T>>> {
                    match self.0.try_slice() {
                        Ok(Some(s)) => Ok(Some(SendSlice(s))),
                        Ok(None) => Ok(None),
                        Err(e) => Err(e),
                    }
                }
            }

            pub struct SendSlice<'a, T>(super::SendSlice<'a, T, $B>);

            impl<'a, T> SendSlice<'a, T> {
                #[inline]
                pub fn push(&mut self, value: T) -> Result<(), PushError<T>> {
                    self.0.push(value)
                }

                #[inline]
                pub fn capacity(&self) -> usize {
                    self.0.capacity()
                }
            }
        }

        pub(super) fn $name<T>(state: State<T, $B>) -> $name::Sender<T> {
            $name::Sender(Sender(state))
        }
    };
}

impl_send!(ring, Ring);
impl_send!(double_ring, DoubleRing);

pub struct Sender<T, B: Behavior>(pub(super) State<T, B>);

impl<T, B: Behavior> Sender<T, B> {
    /*
    pub async fn slice(&mut self) -> Result<SendSlice<T>> {
        poll_fn(|cx| self.poll_slice(cx))
    }

    pub async fn push(&mut self, value: Value) -> Result<()> {
        // TODO
    }
    */

    #[inline]
    pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<SendSlice<T, B>>> {
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
    pub fn try_slice(&mut self) -> Result<Option<SendSlice<T, B>>> {
        Ok(if self.0.acquire_capacity()? {
            let cursor = self.0.cursor;
            Some(SendSlice(&mut self.0, cursor))
        } else {
            None
        })
    }
}

impl<T, B: Behavior> Drop for Sender<T, B> {
    #[inline]
    fn drop(&mut self) {
        if self.0.try_close() {
            self.0.receiver.wake();
        }
    }
}

pub struct SendSlice<'a, T, B: Behavior>(&'a mut State<T, B>, Cursor);

impl<'a, T, B: Behavior> SendSlice<'a, T, B> {
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), PushError<T>> {
        if self.0.cursor.is_full() && !self.0.acquire_capacity()? {
            return Err(PushError::Full(value));
        }

        let (_, pair) = self.0.as_pairs();

        unsafe {
            pair.write(0, value);
        }

        self.0.cursor.increment_tail(1);

        Ok(())
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.0.cursor.send_capacity()
    }
}

impl<'a, T, B: Behavior> Drop for SendSlice<'a, T, B> {
    #[inline]
    fn drop(&mut self) {
        self.0.persist_tail(self.1);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Behavior, Cursor, DoubleRing, Result, Ring, State};
use core::task::{Context, Poll};

macro_rules! impl_recv {
    ($name:ident, $B:ty, $peek:ident, $peek_ty:ty) => {
        pub mod $name {
            use super::*;

            pub struct Receiver<T>(pub(super) super::Receiver<T, $B>);

            impl<T> Receiver<T> {
                #[inline]
                pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<RecvSlice<T>>> {
                    match self.0.poll_slice(cx) {
                        Poll::Ready(Ok(s)) => Poll::Ready(Ok(RecvSlice(s))),
                        Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                        Poll::Pending => Poll::Pending,
                    }
                }

                #[inline]
                pub fn try_slice(&mut self) -> Result<Option<RecvSlice<T>>> {
                    match self.0.try_slice() {
                        Ok(Some(s)) => Ok(Some(RecvSlice(s))),
                        Ok(None) => Ok(None),
                        Err(e) => Err(e),
                    }
                }
            }

            pub struct RecvSlice<'a, T>(super::RecvSlice<'a, T, $B>);

            impl<'a, T> RecvSlice<'a, T> {
                #[inline]
                pub fn peek(&mut self) -> $peek_ty {
                    self.0.$peek()
                }

                #[inline]
                pub fn pop(&mut self) -> Option<T> {
                    self.0.pop()
                }

                #[inline]
                pub fn len(&self) -> usize {
                    self.0.len()
                }

                #[inline]
                pub fn is_empty(&self) -> bool {
                    self.0.is_empty()
                }
            }

            impl<'a, T> Iterator for RecvSlice<'a, T> {
                type Item = T;

                #[inline]
                fn next(&mut self) -> Option<T> {
                    self.pop()
                }
            }
        }

        pub(super) fn $name<T>(state: State<T, $B>) -> $name::Receiver<T> {
            $name::Receiver(Receiver(state))
        }
    };
}

impl_recv!(ring, Ring, peek, (&mut [T], &mut [T]));
impl_recv!(double_ring, DoubleRing, peek_slice, &mut [T]);

pub struct Receiver<T, B: Behavior>(pub(super) State<T, B>);

impl<T, B: Behavior> Receiver<T, B> {
    #[inline]
    pub fn poll_slice(&mut self, cx: &mut Context) -> Poll<Result<RecvSlice<T, B>>> {
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
    pub fn try_slice(&mut self) -> Result<Option<RecvSlice<T, B>>> {
        Ok(if self.0.acquire_filled()? {
            let cursor = self.0.cursor;
            Some(RecvSlice(&mut self.0, cursor))
        } else {
            None
        })
    }
}

impl<T, B: Behavior> Drop for Receiver<T, B> {
    #[inline]
    fn drop(&mut self) {
        if self.0.try_close() {
            self.0.sender.wake();
        }
    }
}

pub struct RecvSlice<'a, T, B: Behavior>(&'a mut State<T, B>, Cursor);

impl<'a, T, B: Behavior> RecvSlice<'a, T, B> {
    #[inline]
    pub fn peek(&mut self) -> (&mut [T], &mut [T]) {
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
    pub fn len(&self) -> usize {
        self.0.cursor.recv_len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.cursor.is_empty()
    }
}

impl<'a, T> RecvSlice<'a, T, DoubleRing> {
    #[inline]
    pub fn peek_slice(&mut self) -> &mut [T] {
        let (slice, _) = self.0.as_slices();
        unsafe { slice.assume_init().into_mut() }
    }
}

impl<'a, T, B: Behavior> Drop for RecvSlice<'a, T, B> {
    #[inline]
    fn drop(&mut self) {
        self.0.persist_head(self.1);
    }
}

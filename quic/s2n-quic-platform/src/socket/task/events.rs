// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::features::Gso;
use core::ops::ControlFlow;

#[derive(Debug)]
pub struct TxEvents {
    count: usize,
    is_blocked: bool,
    gso: Gso,
}

impl TxEvents {
    #[inline]
    pub fn new(gso: Gso) -> Self {
        Self {
            count: 0,
            is_blocked: false,
            gso,
        }
    }

    /// Returns if the task is blocked
    #[inline]
    pub fn is_blocked(&self) -> bool {
        self.is_blocked
    }

    /// Returns if the task was blocked and resets the value
    #[inline]
    pub fn take_blocked(&mut self) -> bool {
        core::mem::take(&mut self.is_blocked)
    }

    /// Sets the task to blocked
    #[inline]
    pub fn blocked(&mut self) {
        self.is_blocked = true;
    }

    /// Returns and resets the number of messages sent
    #[inline]
    pub fn take_count(&mut self) -> usize {
        core::mem::take(&mut self.count)
    }
}

impl crate::syscall::SocketEvents for TxEvents {
    #[inline]
    fn on_complete(&mut self, count: usize) -> ControlFlow<(), ()> {
        // increment the total sent packets and reset our blocked status
        self.count += count;
        self.is_blocked = false;
        ControlFlow::Continue(())
    }

    #[inline]
    fn on_error(&mut self, error: ::std::io::Error) -> ControlFlow<(), ()> {
        use std::io::ErrorKind::*;

        match error.kind() {
            WouldBlock => {
                // record that we're blocked
                self.is_blocked = true;
                ControlFlow::Break(())
            }
            Interrupted => {
                // if we got interrupted break and have the task try again
                ControlFlow::Break(())
            }
            _ => {
                // on platforms that don't support GSO we need to disable it and mark the packet as
                // "sent" even though we weren't able to.
                let _ = self.gso.handle_socket_error(&error);

                // ignore all other errors and just consider the packet sent
                self.count += 1;

                // We `continue` instead of break because it's very unlikely the message would be
                // accepted at a later time, so we just discard the packet.
                ControlFlow::Continue(())
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct RxEvents {
    count: usize,
    is_blocked: bool,
}

impl RxEvents {
    /// Returns if the task is blocked
    #[inline]
    pub fn is_blocked(&self) -> bool {
        self.is_blocked
    }

    /// Returns if the task was blocked and resets the value
    #[inline]
    pub fn take_blocked(&mut self) -> bool {
        core::mem::take(&mut self.is_blocked)
    }

    /// Sets the task to blocked
    #[inline]
    pub fn blocked(&mut self) {
        self.is_blocked = true;
    }

    /// Returns and resets the number of messages sent
    #[inline]
    pub fn take_count(&mut self) -> usize {
        core::mem::take(&mut self.count)
    }
}

impl crate::syscall::SocketEvents for RxEvents {
    #[inline]
    fn on_complete(&mut self, count: usize) -> ControlFlow<(), ()> {
        // increment the total sent packets and reset our blocked status
        self.count += count;
        self.is_blocked = false;
        ControlFlow::Continue(())
    }

    #[inline]
    fn on_error(&mut self, error: ::std::io::Error) -> ControlFlow<(), ()> {
        use std::io::ErrorKind::*;

        match error.kind() {
            WouldBlock => {
                // record that we're blocked
                self.is_blocked = true;
                ControlFlow::Break(())
            }
            Interrupted => {
                // if we got interrupted break and have the task try again
                ControlFlow::Break(())
            }
            _ => {
                // ignore all other errors and have the task try again
                ControlFlow::Break(())
            }
        }
    }
}

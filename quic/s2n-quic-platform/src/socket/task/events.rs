// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// Some of the functions in these impls are not used on non-unix systems
#![cfg_attr(not(unix), allow(dead_code))]

use crate::features::Gso;
use core::ops::ControlFlow;

#[derive(Debug)]
pub struct TxEvents {
    count: usize,
    is_blocked: bool,
    #[cfg_attr(not(s2n_quic_platform_gso), allow(dead_code))]
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

    #[inline]
    pub fn is_blocked(&self) -> bool {
        self.is_blocked
    }

    #[inline]
    pub fn take_blocked(&mut self) -> bool {
        core::mem::take(&mut self.is_blocked)
    }

    #[inline]
    pub fn blocked(&mut self) {
        self.is_blocked = true;
    }

    #[inline]
    pub fn take_count(&mut self) -> usize {
        core::mem::take(&mut self.count)
    }
}

impl crate::syscall::SocketEvents for TxEvents {
    #[inline]
    fn on_complete(&mut self, count: usize) -> ControlFlow<(), ()> {
        self.count += count;
        self.is_blocked = false;
        ControlFlow::Continue(())
    }

    #[inline]
    fn on_error(&mut self, error: ::std::io::Error) -> ControlFlow<(), ()> {
        use std::io::ErrorKind::*;

        match error.kind() {
            WouldBlock => {
                self.is_blocked = true;
                ControlFlow::Break(())
            }
            Interrupted => ControlFlow::Break(()),
            #[cfg(s2n_quic_platform_gso)]
            _ if errno::errno().0 == libc::EIO => {
                self.count += 1;

                self.gso.disable();

                ControlFlow::Continue(())
            }
            _ => {
                self.count += 1;
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
    #[inline]
    pub fn is_blocked(&self) -> bool {
        self.is_blocked
    }

    #[inline]
    pub fn take_blocked(&mut self) -> bool {
        core::mem::take(&mut self.is_blocked)
    }

    #[inline]
    pub fn blocked(&mut self) {
        self.is_blocked = true;
    }

    #[inline]
    pub fn take_count(&mut self) -> usize {
        core::mem::take(&mut self.count)
    }
}

impl crate::syscall::SocketEvents for RxEvents {
    #[inline]
    fn on_complete(&mut self, count: usize) -> ControlFlow<(), ()> {
        self.count += count;
        self.is_blocked = false;
        ControlFlow::Continue(())
    }

    #[inline]
    fn on_error(&mut self, error: ::std::io::Error) -> ControlFlow<(), ()> {
        use std::io::ErrorKind::*;

        match error.kind() {
            WouldBlock => {
                self.is_blocked = true;
                ControlFlow::Break(())
            }
            Interrupted => ControlFlow::Break(()),
            _ => {
                self.count += 1;
                ControlFlow::Break(())
            }
        }
    }
}

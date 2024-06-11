// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{instruction::Dialect, Instruction};
use core::{fmt, mem::size_of};
use libc::sock_fprog;
use std::{io, os::fd::AsRawFd};

pub struct Program<'a, D: Dialect> {
    instructions: &'a [Instruction<D>],
}

impl<'a, D: Dialect> Program<'a, D> {
    #[inline]
    pub const fn new(instructions: &'a [Instruction<D>]) -> Self {
        if instructions.len() > D::MAX_INSTRUCTIONS {
            panic!("program too large");
        }
        Self { instructions }
    }

    #[inline]
    pub fn attach<S: AsRawFd>(&self, socket: &S) -> io::Result<()> {
        let prog = sock_fprog {
            filter: self.instructions.as_ptr() as *const _ as *mut _,
            len: self.instructions.len() as _,
        };

        let ret = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                D::SOCKOPT as _,
                &prog as *const _ as *const _,
                size_of::<sock_fprog>() as _,
            )
        };

        if ret < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl<'a, D: Dialect> fmt::Debug for Program<'a, D> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Program")
            .field("instructions", &self.instructions)
            .finish()
    }
}

impl<'a, D: Dialect> fmt::Display for Program<'a, D> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for inst in self.instructions {
            writeln!(f, "{inst}")?;
        }
        Ok(())
    }
}

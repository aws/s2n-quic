// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{syscall, Result};
use core::fmt;
use std::{
    os::unix::io::{AsRawFd, RawFd},
    sync::Arc,
};

/// A structure for reference counting an AF-XDP socket
#[derive(Clone)]
pub struct Fd(Arc<Inner>);

impl fmt::Debug for Fd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Fd").field(&(self.0).0).finish()
    }
}

impl Fd {
    /// Opens an AF_XDP socket
    ///
    /// This call requires `CAP_NET_RAW` capabilities to succeed.
    #[inline]
    pub fn open() -> Result<Self> {
        let fd = syscall::open()?;
        let fd = Arc::new(Inner(fd));
        Ok(Self(fd))
    }
}

impl AsRawFd for Fd {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        (self.0).0
    }
}

/// Wrap the RawFd in a structure that automatically closes the socket on drop
struct Inner(RawFd);

impl Drop for Inner {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::close(self.0);
        }
    }
}

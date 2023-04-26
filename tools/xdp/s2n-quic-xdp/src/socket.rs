// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{syscall, Result};
use core::fmt;
use std::{
    os::unix::io::{AsRawFd, RawFd},
    sync::Arc,
};

/// A structure for reference counting an AF-XDP socket
#[derive(Clone, PartialEq, Eq)]
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

    pub fn attach_umem(&self, umem: &crate::umem::Umem) -> Result<()> {
        umem.attach(self)?;
        // TODO store the umem
        Ok(())
    }

    /// Returns an invalid file descriptor
    ///
    /// This should only be used in tests to avoid creating an actual socket.
    #[cfg(test)]
    pub fn invalid() -> Self {
        Self(Arc::new(Inner(-1)))
    }
}

impl AsRawFd for Fd {
    #[inline]
    fn as_raw_fd(&self) -> RawFd {
        (self.0).0
    }
}

/// Wrap the RawFd in a structure that automatically closes the socket on drop
#[derive(PartialEq, Eq)]
struct Inner(RawFd);

impl Drop for Inner {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::close(self.0);
        }
    }
}

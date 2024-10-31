// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::probe;
use core::{ffi::CStr, fmt, ops};

#[derive(Debug)]
#[non_exhaustive]
pub struct Info {
    pub id: usize,
    pub name: Str<'static>,
    pub units: Str<'static>,
}

#[doc(hidden)]
pub struct Builder {
    pub id: usize,
    pub name: Str<'static>,
    pub units: Str<'static>,
}

impl Builder {
    #[inline]
    pub const fn build(self) -> Info {
        Info {
            id: self.id,
            name: self.name,
            units: self.units,
        }
    }
}

/// A str that is also a [`CStr`]
#[derive(Clone, Copy)]
pub struct Str<'a>(usize, &'a CStr);

impl<'a> Str<'a> {
    /// # Safety
    ///
    /// The provided value must end in a `\0` character
    pub const unsafe fn new_unchecked(value: &'a str) -> Self {
        unsafe {
            Self(
                value.len() - 1,
                CStr::from_bytes_with_nul_unchecked(value.as_bytes()),
            )
        }
    }
}

impl fmt::Debug for Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl fmt::Display for Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl ops::Deref for Str<'_> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        let len = self.0;
        let ptr = self.1.as_ptr();
        unsafe {
            let bytes = core::slice::from_raw_parts(ptr as *const u8, len);
            core::str::from_utf8_unchecked(bytes)
        }
    }
}

impl AsRef<str> for Str<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

impl AsRef<CStr> for Str<'_> {
    #[inline]
    fn as_ref(&self) -> &CStr {
        self.1
    }
}

impl probe::Arg for Str<'_> {
    #[inline]
    fn into_usdt(self) -> isize {
        self.1.as_ptr() as _
    }
}

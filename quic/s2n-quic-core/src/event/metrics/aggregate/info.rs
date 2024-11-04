// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::probe;
use core::{ffi::CStr, fmt, ops};

#[derive(Debug)]
#[non_exhaustive]
pub struct Info {
    pub id: usize,
    pub name: &'static Str,
    pub units: &'static Str,
}

#[doc(hidden)]
pub struct Builder {
    pub id: usize,
    pub name: &'static Str,
    pub units: &'static Str,
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
#[repr(transparent)]
pub struct Str(str);

impl Str {
    /// Creates a new `Str` value
    ///
    /// # Panics
    ///
    /// The provided slice **must** be nul-terminated and not contain any interior
    /// nul bytes.
    pub const fn new(value: &str) -> &Self {
        {
            let value = value.as_bytes();

            if value.is_empty() {
                panic!("provided string is empty");
            }

            let last_idx = value.len() - 1;

            if value[last_idx] != 0 {
                panic!("string does not end in nul byte");
            }

            let mut idx = 0;
            while idx < last_idx {
                if value[idx] == 0 {
                    panic!("string contains nul byte");
                }
                idx += 1;
            }
        }

        unsafe { Self::new_unchecked(value) }
    }

    /// # Safety
    ///
    /// The provided slice **must** be nul-terminated and not contain any interior
    /// nul bytes.
    pub const unsafe fn new_unchecked(value: &str) -> &Self {
        unsafe {
            // SAFETY: `Self` is `repr(transparent) over a `str``
            core::mem::transmute::<&str, &Self>(value)
        }
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl ops::Deref for Str {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe {
            // SAFETY: string was already checked to contain at least one byte
            assume!(!self.0.is_empty());
        }
        let len = self.0.len() - 1;
        &self.0[..len]
    }
}

impl AsRef<str> for Str {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

impl AsRef<CStr> for Str {
    #[inline]
    fn as_ref(&self) -> &CStr {
        unsafe { CStr::from_bytes_with_nul_unchecked(self.as_bytes()) }
    }
}

impl probe::Arg for &Str {
    #[inline]
    fn into_usdt(self) -> isize {
        self.0.as_ptr() as _
    }
}

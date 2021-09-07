// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;

#[derive(Clone)]
/// Sni holds a negotiated
/// [Server Name Indication](https://en.wikipedia.org/wiki/Server_Name_Indication)
/// value, encoded as UTF-8.
///
/// SNI should be a valid UTF-8 string, therfore this struct can only be
/// constructed from a `&str` or `String`.
///
/// ```rust
/// # use s2n_quic_core::application::Sni;
/// let string: String = String::from("a valid utf-8 string");
/// let sni: Sni = string.into();
///
/// let str_arr: &str = &"a valid utf-8 str array";
/// let sni: Sni = str_arr.into();
/// ```
///
/// `Sni` serves a dual purpose:
/// - It can be converted into [`Bytes`] which supports zero-copy slicing and
/// reference counting.
/// - It can be accessed as `&str` so that applications can reason about the string value.
pub struct Sni(Bytes);

impl Sni {
    #[inline]
    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    #[inline]
    fn as_str(&self) -> &str {
        // Safety: the byte array is validated as a valid UTF-8 string
        // before creating an instance of Sni.
        unsafe { core::str::from_utf8_unchecked(&self.0) }
    }
}

impl From<&str> for Sni {
    #[inline]
    fn from(data: &str) -> Self {
        Sni(Bytes::copy_from_slice(data.as_bytes()))
    }
}

#[cfg(feature = "alloc")]
impl From<alloc::string::String> for Sni {
    #[inline]
    fn from(data: alloc::string::String) -> Self {
        Sni(data.into_bytes().into())
    }
}

impl core::fmt::Debug for Sni {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl core::ops::Deref for Sni {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

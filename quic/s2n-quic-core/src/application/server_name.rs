// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;

/// ServerName holds a negotiated
/// [Server Name Indication](https://en.wikipedia.org/wiki/Server_Name_Indication)
/// value, encoded as UTF-8.
///
/// ServerName should be a valid UTF-8 string, therefore this struct can only be
/// constructed from a `&str` or `String`.
///
/// ```rust
/// # use s2n_quic_core::application::ServerName;
/// let string: String = String::from("a valid utf-8 string");
/// let name: ServerName = string.into();
///
/// let str_arr: &str = &"a valid utf-8 str array";
/// let name: ServerName = str_arr.into();
/// ```
///
/// `ServerName` serves a dual purpose:
/// - It can be converted into [`Bytes`] which supports zero-copy slicing and
/// reference counting.
/// - It can be accessed as `&str` so that applications can reason about the string value.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ServerName(Bytes);

/// A static value for localhost
#[allow(dead_code)] // this is used by conditional modules so don't warn
pub(crate) static LOCALHOST: ServerName = ServerName(Bytes::from_static(b"localhost"));

impl ServerName {
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

impl From<&str> for ServerName {
    #[inline]
    fn from(data: &str) -> Self {
        Self(Bytes::copy_from_slice(data.as_bytes()))
    }
}

#[cfg(feature = "alloc")]
impl From<alloc::string::String> for ServerName {
    #[inline]
    fn from(data: alloc::string::String) -> Self {
        Self(data.into_bytes().into())
    }
}

impl core::fmt::Debug for ServerName {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.as_str().fmt(f)
    }
}

impl core::ops::Deref for ServerName {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;

use super::{ByteVec, ByteVecError};
use core::fmt;
use std::ops;

#[macro_export]
macro_rules! static_bytevec_tag {
    () => {
        s2n_quic_dc::bytevec::static_bytevec_tag!(s2n_quic_dc::bytevec::tagged);

        pub type ByteVec = s2n_quic_dc::bytevec::Tagged<Tag>;
    };
    ($($tagged_path:tt)*) => {
        pub(crate) static COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

        #[derive(Clone, Copy, Debug, Default)]
        pub struct Tag;

        impl Tag {
            pub fn current() -> u64 {
                COUNT.load(core::sync::atomic::Ordering::Relaxed)
            }
        }

        impl $($tagged_path)*::Owner for Tag {
            type Handle = Handle;

            fn tag(&self, len: usize) -> Self::Handle {
                COUNT.fetch_add(len as _, core::sync::atomic::Ordering::Relaxed);
                Handle(len)
            }
        }

        #[derive(Debug)]
        pub struct Handle(usize);

        impl $($tagged_path)*::Handle for Handle {
            fn increment(&mut self, len: usize) {
                if len > 0 {
                    COUNT.fetch_add(len as _, core::sync::atomic::Ordering::Relaxed);
                }
            }

            fn decrement(&mut self, len: usize) {
                if len > 0 {
                    COUNT.fetch_sub(len as _, core::sync::atomic::Ordering::Relaxed);
                }
            }
        }

        impl Clone for Handle {
            fn clone(&self) -> Self {
                COUNT.fetch_add(self.0 as _, core::sync::atomic::Ordering::Relaxed);
                Self(self.0)
            }
        }

        impl Drop for Handle {
            fn drop(&mut self) {
                COUNT.fetch_sub(self.0 as _, core::sync::atomic::Ordering::Relaxed);
            }
        }
    };
}

pub trait Owner: 'static + fmt::Debug {
    type Handle: Handle;

    fn tag(&self, len: usize) -> Self::Handle;
}

pub trait Handle: 'static + fmt::Debug + Clone + Sized {
    fn increment(&mut self, len: usize);
    fn decrement(&mut self, len: usize);
}

#[derive(Debug)]
pub struct Tagged<O: Owner> {
    bytes: ByteVec,
    #[allow(dead_code)]
    tag: O::Handle,
}

impl<O: Owner> Tagged<O> {
    #[inline]
    #[track_caller]
    pub fn new(bytes: ByteVec, owner: &O) -> Self {
        let len = bytes.len();
        let tag = owner.tag(len);
        Self { bytes, tag }
    }

    pub fn push_back(&mut self, bytes: Bytes) {
        self.tag.increment(bytes.len());
        self.bytes.push_back(bytes);
    }

    pub fn append(&mut self, other: &mut ByteVec) {
        self.tag.increment(other.len());
        self.bytes.append(other);
    }

    pub fn split_to(&mut self, at: usize) -> Result<ByteVec, ByteVecError> {
        let chunk = self.bytes.split_to(at)?;
        self.tag.decrement(chunk.len());
        Ok(chunk)
    }

    #[inline]
    pub fn untag(self) -> ByteVec {
        self.bytes
    }

    #[inline]
    pub fn untag_clone(&self) -> ByteVec {
        self.bytes.clone()
    }
}

impl<O: Owner> Clone for Tagged<O> {
    fn clone(&self) -> Self {
        let bytes = self.bytes.clone();
        let tag = self.tag.clone();
        Self { bytes, tag }
    }
}

impl<O: Default + Owner> Default for Tagged<O> {
    #[inline]
    fn default() -> Self {
        Self::new(Default::default(), &Default::default())
    }
}

impl<O: Owner> PartialEq for Tagged<O> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes.eq(&other.bytes)
    }
}

impl<O: Owner> Eq for Tagged<O> {}

impl<O: Default + Owner> From<ByteVec> for Tagged<O> {
    #[inline]
    fn from(value: ByteVec) -> Self {
        Self::new(value, &Default::default())
    }
}

impl<O: Owner> From<Tagged<O>> for ByteVec {
    #[inline]
    fn from(value: Tagged<O>) -> Self {
        value.bytes
    }
}

impl<O: Owner> ops::Deref for Tagged<O> {
    type Target = ByteVec;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod tag_a {
        static_bytevec_tag!(crate::byte_vec::tagged);
    }

    mod tag_b {
        static_bytevec_tag!(crate::byte_vec::tagged);
    }

    #[test]
    fn tag_test() {
        let chunk = ByteVec::from(b"hello!");
        let a: Tagged<tag_a::Tag> = chunk.clone().tag(&tag_a::Tag);
        let b: Tagged<tag_b::Tag> = chunk.tag(&tag_b::Tag);

        assert_eq!(tag_a::Tag::current(), 6);
        assert_eq!(tag_b::Tag::current(), 6);

        let a_clone = a.clone();

        assert_eq!(tag_a::Tag::current(), 12);

        drop(a_clone);

        assert_eq!(tag_a::Tag::current(), 6);

        drop(a);

        assert_eq!(tag_a::Tag::current(), 0);
        assert_eq!(tag_b::Tag::current(), 6);

        drop(b);

        assert_eq!(tag_a::Tag::current(), 0);
        assert_eq!(tag_b::Tag::current(), 0);
    }
}

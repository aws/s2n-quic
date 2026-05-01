// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Storage traits for making packet decoders generic over ownership

use core::ops::{Deref, DerefMut};

/// A type that can store bytes, either borrowed or owned
pub trait Bytes: Deref<Target = [u8]> + DerefMut {}

impl<T: Deref<Target = [u8]> + DerefMut> Bytes for T {}

/// Borrowed byte storage using a mutable slice
pub type BorrowedBytes<'a> = &'a mut [u8];

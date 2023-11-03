// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains abstractions around the platform on which the
//! stack is running

#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

pub mod features;
pub mod io;
pub mod message;
pub mod socket;
pub mod syscall;
#[doc(hidden)] // TODO remove this module: https://github.com/aws/s2n-quic/issues/1738
pub mod time;

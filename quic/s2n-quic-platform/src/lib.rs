// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains abstractions around the platform on which the
//! stack is running

#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

mod features;
pub mod io;
mod message;
mod socket;
mod syscall;
#[doc(hidden)] // TODO remove this module
pub mod time;

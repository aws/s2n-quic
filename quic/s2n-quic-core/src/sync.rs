// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
mod primitive;

#[cfg(feature = "alloc")]
pub mod spsc;

#[cfg(feature = "alloc")]
pub mod worker;

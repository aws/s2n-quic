// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)] // TODO remove once the crate is finished

type Result<T = (), E = std::io::Error> = core::result::Result<T, E>;

/// Primitive types for AF-XDP kernel APIs
mod if_xdp;
/// Helpers for creating mmap'd regions
mod mmap;
/// Structures for tracking ring cursors and synchronizing with the kernel
mod ring;
/// Structure for opening and reference counting an AF-XDP socket
mod socket;
/// Helpers for making API calls to AF-XDP sockets
mod syscall;
/// A shared region of memory for holding frame (packet) data
mod umem;

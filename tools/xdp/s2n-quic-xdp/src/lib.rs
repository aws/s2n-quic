// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)] // TODO remove once the crate is finished

type Result<T = (), E = std::io::Error> = core::result::Result<T, E>;

/// Primitive types for AF-XDP kernel APIs
mod if_xdp;

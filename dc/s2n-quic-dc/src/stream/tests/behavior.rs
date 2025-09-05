// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::duplicate_mod)] // we're loading the same modules twice to test the API behavior

use crate::stream::testing;

mod differential;

#[path = "behavior"]
mod tcp {
    use super::testing::tcp::*;

    mod api;
    mod handshake;
    mod read;
    mod write;
}

#[path = "behavior"]
mod dcquic_tcp {
    use super::testing::dcquic::tcp::*;

    mod api;
    mod handshake;
    mod read;
    mod write;
}

#[path = "behavior"]
#[cfg(not(target_os = "macos"))] // TODO fix macos
mod dcquic_udp {
    use super::testing::dcquic::udp::*;

    mod api;
    mod handshake;
    mod read;
    mod write;
}

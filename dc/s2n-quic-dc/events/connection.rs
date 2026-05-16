// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("stream:write_key_updated")]
pub struct StreamWriteKeyUpdated {
    key_phase: u8,
}

#[event("stream:read_key_updated")]
pub struct StreamReadKeyUpdated {
    key_phase: u8,
}

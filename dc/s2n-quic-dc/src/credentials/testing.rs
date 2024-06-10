// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub fn new(id: u16, key_id: u64) -> Credentials {
    let id = Id((id as u128).to_be_bytes());
    Credentials {
        id,
        key_id: key_id.try_into().unwrap(),
    }
}

pub fn iter(id: u16) -> impl Iterator<Item = Credentials> {
    (0..).map(move |key_id| new(id, key_id))
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub fn new(id: u16, generation_id: u32, sequence_id: u16) -> Credentials {
    let id = Id((id as u128).to_be_bytes());
    Credentials {
        id,
        generation_id,
        sequence_id,
    }
}

pub fn iter(id: u16) -> impl Iterator<Item = Credentials> {
    (0..).map(move |sequence_id| new(id, 0, sequence_id))
}

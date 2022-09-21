// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::transport::parameters::{ClientTransportParameters, ServerTransportParameters};
use bolero::check;
use s2n_codec::assert_codec_round_trip_bytes;

#[test]
fn round_trip() {
    check!().for_each(|input| {
        if input.is_empty() {
            return;
        }

        if input[0] > core::u8::MAX / 2 {
            assert_codec_round_trip_bytes!(ClientTransportParameters, input[1..]);
        } else {
            assert_codec_round_trip_bytes!(ServerTransportParameters, input[1..]);
        }
    });
}

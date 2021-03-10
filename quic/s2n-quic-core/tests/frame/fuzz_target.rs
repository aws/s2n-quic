// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bolero::check;
use s2n_codec::{assert_codec_round_trip_bytes_mut, Encoder, EncoderLenEstimator, EncoderValue};
use s2n_quic_core::frame::FrameRef;

fn main() {
    check!().for_each(|input| {
        let mut input = input.to_vec();
        let frames = assert_codec_round_trip_bytes_mut!(FrameRef, &mut input);

        for frame in frames {
            // make sure the frames encoding size matches what would actually
            // be written to an encoder
            let mut estimator = EncoderLenEstimator::new(core::usize::MAX);
            frame.encode(&mut estimator);
            assert_eq!(frame.encoding_size(), estimator.len());
        }
    });
}

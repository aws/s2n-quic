// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::recovery::RttEstimator;

pub fn rtt_estimator() -> RttEstimator {
    // Set the initial RTT to 2ms so we send a probe after ~6ms
    //
    // TODO longer term, it might be a good idea to have the handshake map
    // entry maintain a recent RTT, or at least use the value from the original
    // handshake. This default value is going to be difficult to get right
    // for every environment.
    RttEstimator::new(core::time::Duration::from_millis(2))
}

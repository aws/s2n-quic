// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::ack;

#[derive(Debug, Default)]
pub struct Report {
    pub client: EndpointReport,
    pub server: EndpointReport,
    pub iterations: usize,
}

#[derive(Debug, Default)]
pub struct EndpointReport {
    /// Final state of the pending AckRanges
    pub pending_ack_ranges: ack::Ranges,
    /// Total number of transmissions sent
    pub total_transmissions: usize,
    /// Number of transmissions that elicited an ACK
    pub ack_eliciting_transmissions: usize,
    /// Number of transmissions that contained an ACK frame
    pub ack_transmissions: usize,
    /// Number of transmissions that experienced congestion
    pub congested_transmissions: usize,
    /// Number of transmissions that were dropped by the network
    pub dropped_transmissions: usize,
    /// Number of transmissions that were delayed by the network
    pub delayed_transmissions: usize,
    /// Number of transmissions that were processed by the peer
    pub processed_transmissions: usize,
}

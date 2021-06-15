// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14
//# The maximum datagram size MUST be at least 1200 bytes.
pub const MINIMUM_MTU: u16 = 1200;

// TODO decide on better defaults
pub const DEFAULT_MAX_MTU: u16 = 1500;

// Initial PTO backoff multiplier is 1 indicating no additional increase to the backoff.
pub const INITIAL_PTO_BACKOFF: u32 = 1;

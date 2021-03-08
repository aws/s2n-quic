// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::crypto::{HandshakeHeaderKey, HandshakeKey};

header_key!(RingHandshakeHeaderKey);
negotiated_crypto!(RingHandshakeKey, RingHandshakeHeaderKey);

impl HandshakeKey for RingHandshakeKey {}

impl HandshakeHeaderKey for RingHandshakeHeaderKey {}

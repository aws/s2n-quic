// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::crypto;

header_key!(HandshakeHeaderKey);
negotiated_crypto!(HandshakeKey, HandshakeHeaderKey);

impl crypto::HandshakeKey for HandshakeKey {}

impl crypto::HandshakeHeaderKey for HandshakeHeaderKey {}

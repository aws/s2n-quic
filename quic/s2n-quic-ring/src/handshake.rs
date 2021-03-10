// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::crypto::HandshakeCrypto;

negotiated_crypto!(RingHandshakeCrypto);

impl HandshakeCrypto for RingHandshakeCrypto {}

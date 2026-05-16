// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_platform::features;

#[derive(Clone)]
pub struct Gso<S>(pub S, pub features::Gso);

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn entry_size() {
    let mut should_check = true;

    should_check &= cfg!(target_pointer_width = "64");
    should_check &= cfg!(target_os = "linux");
    should_check &= std::env::var("S2N_QUIC_RUN_VERSION_SPECIFIC_TESTS").is_ok();

    // This gates to running only on specific GHA to reduce false positives.
    if should_check {
        assert_eq!(
            Entry::fake((std::net::Ipv4Addr::LOCALHOST, 0).into(), None).size(),
            311
        );
    }
}

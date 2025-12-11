// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// This has a very much shorter list than the actual enum - this is intentional, it tests that we
// handle non_exhaustive correctly.
counters_for_enum! {
    enum std::io::ErrorKind as ErrorKindCounters {
        NotFound,
        PermissionDenied,
    }
}

#[test]
fn counters_for_enum_counts() {
    let registry = crate::Registry::new();
    let counters = ErrorKindCounters::new("FileOpen", "IO", &registry);
    counters.count(&std::io::ErrorKind::ConnectionReset);
    counters.count(&std::io::ErrorKind::NotFound);
    counters.count(&std::io::ErrorKind::NotFound);

    assert_eq!(
        registry.take_current_metrics_line(),
        "FileOpen=2 Variant|IO-NotFound,FileOpen=1 Variant|IO-Other,FileOpen=0 Variant|IO-PermissionDenied"
    );
}

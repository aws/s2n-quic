// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[event("count_event")]
#[subject(endpoint)]
struct CountEvent {
    count: u32,
}

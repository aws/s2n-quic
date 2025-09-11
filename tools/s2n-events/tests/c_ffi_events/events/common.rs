// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

enum Subject {
    Endpoint,
    Connection {
        id: u64,
    },
}

struct ConnectionMeta {
    id: u64,
    timestamp: Timestamp,
}

struct EndpointMeta {}

struct ConnectionInfo {}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Trait for a metric than can be queried on a server
pub trait Metric {
    // TODO
}

/// The total number of connections for the server
pub struct ConnectionsTotal(pub usize);

/// The number of open connections for the server
pub struct ConnectionsOpen(pub usize);

/// The number of closed connections for the server
pub struct ConnectionsClosed(pub usize);

/// The number of bytes sent on the server
pub struct BytesSent(pub usize);

/// The number of bytes received on the server
pub struct BytesReceived(pub usize);

impl Metric for ConnectionsTotal {}
impl Metric for ConnectionsOpen {}
impl Metric for ConnectionsClosed {}
impl Metric for BytesSent {}
impl Metric for BytesReceived {}

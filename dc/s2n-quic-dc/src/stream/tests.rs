// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod accept_queue;
mod behavior;
/// A set of tests ensuring we support a large number of peers.
#[cfg(future)] // TODO remove this since they're quite expensive
mod cardinality;
mod deterministic;
mod idle_timeout;
mod key_update;
mod request_response;
mod restart;
mod rpc;

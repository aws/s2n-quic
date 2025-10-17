// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{testing, Protocol};

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
mod shared_cache;

/// Shows an endpoint doesn't need an application tokio runtime to be created
#[test]
fn runtime_free_context_test() {
    let _ = testing::dcquic::Context::new_sync(Protocol::Udp, "127.0.0.1:0".parse().unwrap());
}

mod sizes {
    use core::mem::size_of;

    #[test]
    fn stream_test() {
        assert_eq!(
            size_of::<crate::stream::testing::Stream>(),
            size_of::<Box<()>>() * 2
        );
    }

    #[test]
    fn reader_test() {
        assert_eq!(
            size_of::<crate::stream::testing::Reader>(),
            size_of::<Box<()>>()
        );
    }

    #[test]
    fn writer_test() {
        assert_eq!(
            size_of::<crate::stream::testing::Writer>(),
            size_of::<Box<()>>()
        );
    }
}

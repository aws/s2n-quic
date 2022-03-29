// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::Poll;
use s2n_quic::provider::tls::s2n_tls::{ClientHelloHandler, Connection};

pub struct MyClientHelloHandler {}

impl ClientHelloHandler for MyClientHelloHandler {
    fn poll_client_hello(&self, connection: &mut Connection) -> core::task::Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
}

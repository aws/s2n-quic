// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::testing::dcquic::tcp::*;

#[tokio::test]
async fn context_accessible() {
    let context = Context::new().await;
    let (mut client, _server) = context.pair().await;

    // Confirms that the () context on NoopSubscriber is accessible.
    client.query_event_context(|_: &()| ()).unwrap();
    let (read, write) = client.split();
    read.query_event_context(|_: &()| ()).unwrap();
    write.query_event_context(|_: &()| ()).unwrap();
}

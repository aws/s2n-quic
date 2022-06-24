// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench_scenarios::prelude::*;

config!({
    /// The number of separate connections to create
    let connections: u64 = 1000;
});

pub fn scenario(config: Config) -> Scenario {
    let Config { connections } = config;

    Scenario::build(|scenario| {
        let server = scenario.create_server();

        scenario.create_client(|client| {
            for _ in 0..connections {
                client.connect_to(&server, |conn| {
                    conn.open_bidirectional_stream(
                        |local| {
                            local.send(1.bytes());
                            local.receive(1.bytes());
                        },
                        |remote| {
                            remote.receive(1.bytes());
                            remote.send(1.bytes());
                        },
                    );
                });
            }
        });
    })
}

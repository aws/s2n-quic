// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench_scenarios::prelude::*;

config!({
    /// The number of concurrent connections to create
    let connections: u64 = 1;

    /// The number of concurrent streams to ping on
    let streams: u64 = 1;

    /// The amount of time to spend pinging for each size
    let time: Duration = 15.seconds();

    /// The amount of data to send in each ping
    let size: Vec<Byte> = vec![
        1000.bytes(),
        10_000.bytes(),
        100_000.bytes(),
        1_000_000.bytes(),
    ];
});

pub fn scenario(config: Config) -> Scenario {
    let Config {
        connections,
        streams,
        size: sizes,
        time,
    } = config;

    Scenario::build(|scenario| {
        let server = scenario.create_server();

        scenario.create_client(|client| {
            for size in sizes.iter().copied() {
                let ping = format!("ping {size}");
                let pong = format!("pong {size}");
                client.scope(|client| {
                    for _ in 0..connections {
                        client.spawn(|client| {
                            client.connect_to(&server, |conn| {
                                conn.scope(|conn| {
                                    for _ in 0..streams {
                                        conn.spawn(|conn| {
                                            conn.open_bidirectional_stream(
                                                |local| {
                                                    local.iterate(time, |local| {
                                                        local.profile(&ping, |local| {
                                                            local.send(size);
                                                            local.receive(size);
                                                        });
                                                    });
                                                },
                                                |remote| {
                                                    remote.iterate(time, |remote| {
                                                        remote.profile(&pong, |remote| {
                                                            remote.receive(size);
                                                            remote.send(size);
                                                        });
                                                    });
                                                },
                                            );
                                        });
                                    }
                                });
                            });
                        });
                    }
                });
            }
        });
    })
}

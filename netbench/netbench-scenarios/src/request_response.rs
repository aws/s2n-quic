// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench_scenarios::prelude::*;

config!({
    /// The size of the client's request to the server
    let request_size: Byte = 1.kilobytes();

    /// The size of the server's response to the client
    let response_size: Byte = 10.megabytes();

    /// How long the server will take to respond to the request
    let response_delay: Duration = 0.seconds();

    /// The number of requests to make
    let count: u64 = 1;

    /// The number of separate connections to create
    let connections: u64 = 1;

    /// Specifies if the requests should be performed in parallel
    let parallel: bool = false;

    /// The rate at which the client sends data
    let client_send_rate: Option<Rate> = None;

    /// The rate at which the client receives data
    let client_receive_rate: Option<Rate> = None;

    /// The rate at which the server sends data
    let server_send_rate: Option<Rate> = None;

    /// The rate at which the server receives data
    let server_receive_rate: Option<Rate> = None;

    /// The number of bytes that must be received before the next request
    let response_unblock: Byte = 0.bytes();
});

pub fn scenario(config: Config) -> Scenario {
    let Config {
        request_size,
        response_size,
        count,
        connections,
        parallel,
        client_send_rate,
        client_receive_rate,
        server_send_rate,
        server_receive_rate,
        response_delay,
        response_unblock,
    } = config;
    let response_unblock = response_unblock.min(response_size);

    type Checkpoint = Option<
        builder::checkpoint::Checkpoint<builder::Client, builder::Local, builder::checkpoint::Park>,
    >;

    let request = |conn: &mut builder::connection::Builder<builder::Client>,
                   checkpoint: &mut Checkpoint| {
        let (park, unpark) = conn.checkpoint();

        if let Some(park) = checkpoint.take() {
            conn.park(park);
        }

        conn.open_bidirectional_stream(
            |local| {
                if let Some(rate) = client_send_rate {
                    local.set_send_rate(rate);
                }
                if let Some(rate) = client_receive_rate {
                    local.set_receive_rate(rate);
                }
                local.send(request_size);

                if *response_unblock > 0 {
                    local.receive(response_unblock);
                    local.unpark(unpark);
                    local.receive(response_size - response_unblock);
                } else {
                    local.receive(response_size);
                }
            },
            |remote| {
                if let Some(rate) = server_send_rate {
                    remote.set_send_rate(rate);
                }
                if let Some(rate) = server_receive_rate {
                    remote.set_receive_rate(rate);
                }
                remote.receive(request_size);

                if response_delay != Duration::ZERO {
                    remote.sleep(response_delay);
                }

                remote.send(response_size);
            },
        );

        if *response_unblock > 0 {
            *checkpoint = Some(park)
        }
    };

    Scenario::build(|scenario| {
        let server = scenario.create_server();

        scenario.create_client(|client| {
            for _ in 0..connections {
                client.connect_to(&server, |conn| {
                    if parallel {
                        conn.scope(|scope| {
                            let mut prev_checkpoint = None;
                            for _ in 0..count {
                                scope.spawn(|conn| {
                                    request(conn, &mut prev_checkpoint);
                                });
                            }
                        });
                    } else {
                        for _ in 0..count {
                            request(conn, &mut None);
                        }
                    }
                });
            }
        });
    })
}

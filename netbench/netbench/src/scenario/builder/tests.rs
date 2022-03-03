// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    scenario::{Builder, Scenario},
    units::*,
};
use insta::assert_json_snapshot;

fn scenario<F: FnOnce(&mut Builder)>(f: F) -> Scenario {
    let mut scenario = Scenario::build(f);
    // hash traversal order was changed somewhere between 1.53 and 1.58 so
    // we won't compare the id for now
    scenario.id = Default::default();
    // cert keys are not deterministic
    scenario.certificates.clear();
    scenario
}

macro_rules! scenario_test {
    ($name:ident, $builder:expr) => {
        #[test]
        fn $name() {
            assert_json_snapshot!(scenario($builder));
        }
    };
}

scenario_test!(simple, |scenario| {
    let server = scenario.create_server();

    scenario.create_client(|client| {
        client.connect_to(server, |conn| {
            conn.open_send_stream(
                |local| {
                    local.set_send_rate(1024.bytes() / 50.millis());
                    local.send(1.megabytes());
                },
                |peer| {
                    peer.set_receive_rate(1024.bytes() / 50.millis());
                    peer.receive(1.megabytes());
                },
            );
        });
    });
});

scenario_test!(conn_checkpoints, |scenario| {
    let server = scenario.create_server();

    scenario.create_client(|client| {
        client.connect_to(server, |conn| {
            let (cp1_rx, cp1_tx) = conn.checkpoint();

            conn.concurrently(
                |conn| {
                    conn.open_send_stream(
                        |local| {
                            local.set_send_rate(10.kilobytes() / 50.millis());
                            local.send(1.megabytes() / 2);
                            local.unpark(cp1_tx);
                            local.send(1.megabytes() / 2);
                        },
                        |peer| {
                            peer.set_receive_rate(10.kilobytes() / 50.millis());
                            peer.receive(1.megabytes());
                        },
                    );
                },
                |conn| {
                    conn.open_send_stream(
                        |local| {
                            local.park(cp1_rx);
                            local.set_send_rate(1024.bytes() / 50.millis());
                            local.send(1.megabytes());
                        },
                        |peer| {
                            peer.set_receive_rate(1024.bytes() / 50.millis());
                            peer.receive(1.megabytes());
                        },
                    );
                },
            );
        });
    });
});

scenario_test!(linked_streams, |scenario| {
    let server = scenario.create_server();

    scenario.create_client(|client| {
        client.connect_to(server, |conn| {
            let (b_park, b_unpark) = conn.checkpoint();

            conn.concurrently(
                |conn| {
                    conn.open_bidirectional_stream(
                        |local| {
                            local.concurrently(
                                |sender| {
                                    sender.set_send_rate(1024.bytes() / 50.millis());
                                    sender.send(1.megabytes());
                                },
                                |receiver| {
                                    receiver.receive_all();
                                },
                            );

                            local.unpark(b_unpark);
                        },
                        |peer| {
                            peer.sleep(100.millis());

                            peer.set_receive_rate(1024.bytes() / 50.millis());
                            peer.receive(100.kilobytes());

                            peer.set_receive_rate(10.bytes() / 50.millis());
                            peer.receive_all();

                            peer.send(2.megabytes());
                        },
                    );
                },
                |conn| {
                    conn.open_send_stream(
                        |local| {
                            local.park(b_park);
                            local.send(1.megabytes());
                        },
                        |peer| {
                            peer.receive(1.megabytes());
                        },
                    );
                },
            );
        });
    });
});

scenario_test!(custom_cert, |scenario| {
    let ca = scenario.create_ca();

    let server = scenario.create_server_with(|server| {
        server.set_cert(ca.key_pair());
    });

    scenario.create_client(|client| {
        client.connect_to(server, |conn| {
            conn.open_send_stream(
                |local| {
                    local.set_send_rate(1024.bytes() / 50.millis());
                    local.send(1.megabytes());
                },
                |peer| {
                    peer.set_receive_rate(1024.bytes() / 50.millis());
                    peer.receive(1.megabytes());
                },
            );
        });
    });
});

scenario_test!(long_chain_cert, |scenario| {
    let ca = scenario.create_ca_with(|ca| {
        ca.ecdsa();
    });

    let server = scenario.create_server_with(|server| {
        let key = ca.key_pair_with(|key| {
            key.push_ia();
            key.push_ia_with(|ia| {
                ia.ed25519();
            });
        });
        server.set_cert(key);
    });

    scenario.create_client(|client| {
        client.connect_to(server, |conn| {
            conn.open_send_stream(
                |local| {
                    local.set_send_rate(1024.bytes() / 50.millis());
                    local.send(1.megabytes());
                },
                |peer| {
                    peer.set_receive_rate(1024.bytes() / 50.millis());
                    peer.receive(1.megabytes());
                },
            );
        });
    });
});

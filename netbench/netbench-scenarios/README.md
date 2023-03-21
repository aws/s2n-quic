# netbench-scenarios

The executable includes three default scenarios
- [`request response`](https://github.com/aws/s2n-quic/blob/main/netbench/netbench-scenarios/src/request_response.rs) sends `N` number of bytes to the server, which responds with `M` number of bytes.
- [`ping`](https://github.com/aws/s2n-quic/blob/main/netbench/netbench-scenarios/src/ping.rs) will "ping-pong" a data payload from client to the server and back
- [`connect`](https://github.com/aws/s2n-quic/blob/main/netbench/netbench-scenarios/src/connect.rs) will open a number of connections and then exchange a single byte. This is useful for evaluating connection setup times.


Several options are available for configuration:

```shell
$ cargo run --bin netbench-scenarios -- --help

netbench scenarios

USAGE:
    netbench-scenarios [FLAGS] [OPTIONS] [OUT_DIR]

FLAGS:
    -h, --help
            Prints help information

        --request_response.parallel
            Specifies if the requests should be performed in parallel

    -V, --version
            Prints version information

OPTIONS:
        --connect.connections <COUNT>
            The number of separate connections to create [default: 1000]

        --ping.connections <COUNT>
            The number of concurrent connections to create [default: 1]

        --ping.size <BYTES>
            The amount of data to send in each ping [default: 1KB,10KB,100KB,1MB]

        --ping.streams <COUNT>
            The number of concurrent streams to ping on [default: 1]

        --ping.time <TIME>
            The amount of time to spend pinging for each size [default: 15s]

        --request_response.client_receive_rate <RATE>
            The rate at which the client receives data [default: NONE]

        --request_response.client_send_rate <RATE>
            The rate at which the client sends data [default: NONE]

        --request_response.connections <COUNT>
            The number of separate connections to create [default: 1]

        --request_response.count <COUNT>
            The number of requests to make [default: 1]

        --request_response.request_size <BYTES>
            The size of the client's request to the server [default: 1KB]

        --request_response.response_delay <TIME>
            How long the server will take to respond to the request [default: 0s]

        --request_response.response_size <BYTES>
            The size of the server's response to the client [default: 10MB]

        --request_response.response_unblock <BYTES>
            The number of bytes that must be received before the next request [default: 0B]

        --request_response.server_receive_rate <RATE>
            The rate at which the server receives data [default: NONE]

        --request_response.server_send_rate <RATE>
            The rate at which the server sends data [default: NONE]


ARGS:
    <OUT_DIR>
             [default: target/netbench]

FORMATS:
    BYTES
        42b         ->    42 bits
        42          ->    42 bytes
        42B         ->    42 bytes
        42K         ->    42000 bytes
        42Kb        ->    42000 bits
        42KB        ->    42000 bytes
        42KiB       ->    43008 bytes

    COUNT
        42          ->    42 units

    RATE
        42bps       ->    42 bits per second
        42Mbps      ->    42 megabits per second
        42MBps      ->    42 megabytes per second
        42MiBps     ->    42 mebibytes per second
        42MB/50ms   ->    42 megabytes per 50 milliseconds

    TIME
        42ms         ->    42 milliseconds
        42s          ->    42 seconds
        1s42ms       ->    1 second + 42 milliseconds
```

Moving forward, we can add any useful scenarios to this list.

### Library

For workflows that want to build their own scenarios, it can depend on the library and set up their `main.rs` as follows:

```rust
netbench_scenario::scenarios!(my_scenario_a, my_scenario_b);
```

They would then create a `my_scenario_a.rs` and `my_scenario_b.rs`:

```rust
// my_scenario_a.rs
use netbench_scenario::prelude::*;

config!({
    /// The size of the client's request to the server
    let request_size: Byte = 1.kilobytes();

    /// The size of the server's response to the client
    let response_size: Byte = 10.megabytes();
});

pub fn scenario(config: Config) -> Scenario {
    let Config {
        request_size,
        response_size,
    } = config;

    Scenario::build(|scenario| {
        let server = scenario.create_server();

        scenario.create_client(|client| {
            client.connect_to(server, |conn| {
                conn.open_bidirectional_stream(
                    |local| {
                        local.send(request_size);
                        local.receive(response_size);
                    },
                    |remote| {
                        remote.receive(request_size);
                        remote.send(response_size);
                    },
                );
            });
        });
    })
}
```

They can then run their scenario generator:

```shell
$ cargo run -- --help

netbench scenarios

USAGE:
    netbench-scenarios [FLAGS] [OPTIONS] [OUT_DIR]

FLAGS:
    -h, --help
            Prints help information

    -V, --version
            Prints version information


OPTIONS:
        --my_scenario_a.request_size <BYTES>
            The size of the client's request to the server [default: 1KB]

        --my_scenario_a.response_size <BYTES>
            The size of the server's response to the client [default: 10MB]


ARGS:
    <OUT_DIR>
             [default: target/netbench]

```

```
$ cargo run
created: target/netbench/my_scenario_a.json
created: target/netbench/my_scenario_b.json
```

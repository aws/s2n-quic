# netbench-scenarios

### Executable

The executable includes a single default scenario: [`request_response`](https://github.com/aws/s2n-quic/blob/main/netbench/netbench-scenarios/src/request_response.rs). This sends `N` number of bytes to the server, which responds with `M` number of bytes. Several options are available for configuration:

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
        --request_response.client_receive_rate <RATE>
            The rate at which the client receives data [default: NONE]

        --request_response.client_send_rate <RATE>
            The rate at which the client sends data [default: NONE]

        --request_response.count <COUNT>
            The number of requests to make [default: 1]

        --request_response.request_size <BYTES>
            The size of the client's request to the server [default: 1KB]

        --request_response.response_delay <TIME>
            How long the server will take to respond to the request [default: 0s]

        --request_response.response_size <BYTES>
            The size of the server's response to the client [default: 10MB]

        --request_response.server_receive_rate <RATE>
            The rate at which the server receives data [default: NONE]

        --request_response.server_send_rate <RATE>
            The rate at which the server sends data [default: NONE]


ARGS:
    <OUT_DIR>
             [default: target/netbench]
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

The can then run their scenario generator:

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
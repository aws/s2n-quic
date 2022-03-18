# netbench

A efficiency, performance, and correctness analysis tool for transport protocols.

## Why does this exist?

There are many transport protocols and several implementations of each. This tool exists to provide users with the ability to perform a direct comparison and decide the best implementation for their workloads.

Here are a few examples of questions that netbench aims to answer:

* What is the cost of encrypting traffic?
    * How much more capacity will I need when deploying this?
* What transport protocol performs best
    * in a data center?
    * in networks with high packet loss?
    * in networks with high latency?
    * with many concurrent, multiplexed streams?
* Which implementation of "X" protocol is best for my workload?
* What is the optimal configuration of the transport's settings for my workload?
* How does certificate chain length affect handshake throughput?
* Is implementation "X" interoperable with implementation "Y" of "Z" protocol?

## How it works

`netbench` provides tools to write [scenarios](./netbench-scenarios/) that describe application workloads. An example of a scenario is a simple request/response pattern between a client and server:

```rust
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

This scenario generates a json file of instructions. These instructions are protocol and language independent, which means they can easily be executed by a ["netbench driver"](./netbench-driver/), written in any language or runtime. Implemented drivers include:

* `TCP`
* [`native-tls`](https://crates.io/crates/native-tls)
    * OpenSSL on Linux
    * Secure Transport on macOS
    * SChannel on Windows
* `s2n-quic`
* `s2n-tls` (coming soon)

Driver metrics are collected with the [`netbench-collector`](./netbench-collector/) utility. Reports are then generated for the collected metrics with the [`cli`](./netbench-cli/).

A [sample report can be found here](https://dnglbrstg7yg.cloudfront.net/8e1890f04727ef7d3acdcb521c5b3cda257778f0/netbench/index.html#request_response/clients.json).
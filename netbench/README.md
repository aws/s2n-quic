# netbench

An efficiency, performance, and correctness analysis tool for transport protocols.

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

## Quickstart
A basic use of netbench is demonstrated in the `run-netbench-request-response.sh` script. This script will
- compile all necessary netbench utilities
- generate scenario files
- execute the `request-response.json` scenario file using `s2n-quic` drivers
- collect statistics from the drivers using `netbench-collector`
- generate a report in the `./report` directory

```
./run-netbench-request-response.sh
cd report
python3 -m http.server 9000
```
Then navigate to `localhost:9000` in a browser to view the netbench results.

## How it works

### netbench-scenarios
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

This scenario generates a json file of instructions. These instructions are protocol and language independent, which means they can easily be executed by a ["netbench driver"](./netbench-driver/), written in any language or runtime.

### netbench-driver
Netbench drivers are responsible for executing netbench scenarios. Each transport protocol has a `client` and `server` implementation. Each of these implementations is a self-container binary that consumes a `scenario.json` file. Implemented drivers include:

* `TCP`
* [`native-tls`](https://crates.io/crates/native-tls)
    * OpenSSL on Linux
    * Secure Transport on macOS
    * SChannel on Windows
* `s2n-quic`
* `s2n-tls`

### netbench-collector
Driver metrics are collected with the [`netbench-collector`](./netbench-collector/) utility. There are two implementation of this available - a generic utility and a bpftrace utility. The generic utility uses the `proc fs` to gather information about the process, while the `bpftrace` implementation is able to collect a wider variety of statistics through ebpf probes.

The collector binary takes a `netbench-driver` as an argument. The driver binary is spawned as a child process. The collector will continuously gather metrics from the driver and emit those metrics to `stdout`.

### netbench-cli
`netbench-cli` has two commands.

`report` is used to generate individual `.json` reports, and is not expected to be useful in isolation.

`report-tree` is used to to generate a human-readable `.html` report. Given a directory structure like the following
```
request-response/ # scenario
├─ tls/ # driver
│  ├─ client.json
│  ├─ server.json
├─ quic/
   ├─ client.json
   ├─ server.json
```
`report-tree` will generate the individual `reports` and package them into a human readable `index.html` file that can be used to view graphs of the results.

A [sample report can be found here](https://dnglbrstg7yg.cloudfront.net/8e1890f04727ef7d3acdcb521c5b3cda257778f0/netbench/index.html#request_response/clients.json).

Note that you will not be able to open the report directly since the report relies on the jsdelivr cdn. This request will fail when the URL is a local file scheme with a [CORS request not HTTP](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS/Errors/CORSRequestNotHttp) error.

To get around this, use a local server.
```
# assuming the report is in ./report
cd report
# start a local server on port 9000
python3 -m http.server 9000
```
In a browser, navigate to `localhost:9000` to view the netbench report.
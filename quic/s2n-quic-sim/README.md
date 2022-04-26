# s2n-quic-sim

A Monte Carlo simulation environment for s2n-quic

## Getting started

Simulation plans are specified in a toml file:

```toml
[sim]
# vary the number of inflight packets allowed from 1 to 1000
max_inflight = "1..1000"
# open 100 connections
connections = 100
# perform the simulation 10,000 times
iterations = 10000
# send 100kb of data on each connection
stream_data = 100000

# generate a report comparing max_inflight to connection duration
[report.duration]
filters = ["conn.success"]
x = "net.max_inflight"
y = "conn.duration"

# generate a report comparing max_inflight to number of packets sent on each connection
[report.tx-packets]
filters = ["conn.success"]
x = "net.max_inflight"
y = "conn.tx.packets"
```

Plans can be executed with the `batch` command:

```
cd quic/s2n-quic-sim
cargo run --release -- batch ./path/to/plan.toml
```

## How the simulation works

`s2n-quic-sim` uses a [Monte Carlo method](https://en.wikipedia.org/wiki/Monte_Carlo_method) to explore the execution bounds of `s2n-quic`. From [Wikipedia](https://en.wikipedia.org/wiki/Monte_Carlo_method#Overview):

> Monte Carlo methods vary, but tend to follow a particular pattern:
>
> * Define a domain of possible inputs
> * Generate inputs randomly from a probability distribution over the domain
> * Perform a deterministic computation on the inputs
> * Aggregate the results

In the case of `s2n-quic-sim`, the input domains are specified in the simulation plan. The simulation iterates the specified number of times and metrics are collected. After the simulation completes, one or more reports are generated. Usually these reports compare the input value to some output value. For example, if we vary the network's  and wanted to understand the impact it had on connection

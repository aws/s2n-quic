Netbench Test Player

Netbench collector is designed to collect data on the performance of a
a implementation of a transport protocol. The transport protocol
implementation exists outside of netbench collector in a separate binary
known as a driver (see netbench-driver for some examples).

To run netbench collector you will need a netbench scenario
(see netbench-scenarios) and a driver at a minimum:

```bash
netbench-collector netbench-driver-s2n-quic-server --scenario SCENARIO.json > client_results.json &
```

You'll also need a client to connect to the server and put it under load
so we have something interesting to test.

(By default the netbench server binds to localhost:4433)

```bash
SERVER_0=localhost:4433 netbench-collector netbench-driver-s2n-quic-client --scenario SCENARIO.json > server_results.json
```

The results can be post processed with netbench-cli to generate a report.

Notice in the above example that we intentionally start the server first.
If we had started the client without the server running it would stop
immediately after failing to connect to the server.

After the client finishes you'll also need to kill the server.

It is easy to get the order right, and to manually kill the server when doing
testing by hand. But in the context of a CI or test automation across multiple
machines we may not be able to ensure that servers and clients come up in the
right order and ideally we'd like the server to cleanly quit when the client is
finished. Netbench test player extends the options of netbench collector to
achieve this coordination.

In the following example we use netbench-test-player's to
run the same test as above. Netbench Test player is now responsible for both coordinating
the start and end of these tests. Both processes will quit on their own when finished.
Each process will wait for the other regardless of the order in which they are started
(notice in this example the client is started first).

```
SERVER_0=localhost:4433 netbench-test-player --run-as client \
    --remote-status-server "localhost:8081" \
    --local-status-port 8082 \
    netbench-driver-s2n-quic-client \
     --scenario SCENARIO.json \
     > client_results.json &

netbench-test-player --run-as server \
    --remote-status-server "localhost:8082" \
    --local-status-port 8081 \
    netbench-driver-s2n-quic-server \
    --scenario SCENARIO.json \
    > server_results.json &
```

The coordination here is done on a separate port: 8081 & 8082. In this case
both happen to be on localhost.
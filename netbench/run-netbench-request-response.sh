#!/usr/bin/env bash

#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# build the scenario generation tool
cargo build --manifest-path netbench-scenarios/Cargo.toml

# generate the scenario files. This will generate .json files that can be found
# in the netbench/target/netbench directory. Specify that there should be 1000
# request-response occurrences
cargo run --manifest-path netbench-scenarios/Cargo.toml -- --request_response.count 1000

# build the drivers
cargo build --manifest-path netbench-driver/Cargo.toml --release

# build the statistics collectors
cargo build --manifest-path netbench-collector/Cargo.toml --release

# build the netbench cli
cargo build --manifest-path netbench-cli/Cargo.toml --release

# make a directory to hold the collected statistics
mkdir -p results/request_response/s2n-quic/

# run the server while collecting metrics. Generic Segmenetation Offload (GSO)
# is disabled, because it results in production-disssimilar behaviors when
# running over the loopback interface. Specifically, packets larger than the
# max supported MTU can be observed.
echo "running the server"
DISABLE_GSO=true ./target/release/netbench-collector ./target/release/netbench-driver-s2n-quic-server --scenario ./target/netbench/request_response.json > results/request_response/s2n-quic/server.json &
# store the server process' PID. "$!" is the more recently spawns child pid
SERVER_PID=$!

# run the client. Port 4433 is the default for the server.
echo "running the client"
echo "the scenario should take about 15 seconds to run"
DISABLE_GSO=true SERVER_0=localhost:4433 ./target/release/netbench-collector ./target/release/netbench-driver-s2n-quic-client --scenario ./target/netbench/request_response.json > results/request_response/s2n-quic/client.json

# cleanup server processes. The collector PID (which is the parent) is stored in
# SERVER_PID. The collector forks the driver process. The following incantation
# kills the child processes as well.
echo "killing the server"
kill $(ps -o pid= --ppid $SERVER_PID)

echo "generating the report"
./target/release/netbench-cli report-tree results report

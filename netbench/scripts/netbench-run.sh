#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

ARTIFACT_FOLDER="target/release"
NETBENCH_ARTIFACT_FOLDER="target/netbench"

# the run_trial function will run the request-response scenario
# with the driver passed in as the first argument
run_trial() {
    # e.g. request-response
    SCENARIO=$1
    # e.g. s2n-quic
    DRIVER=$2
    echo "running the $SCENARIO scenario with $DRIVER"

    # make a directory to hold the collected statistics
    mkdir -p $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER

    # run the server while collecting metrics.
    echo "  running the server"
    ./$ARTIFACT_FOLDER/netbench-collector \
    ./$ARTIFACT_FOLDER/netbench-driver-$DRIVER-server \
    --scenario ./$NETBENCH_ARTIFACT_FOLDER/$SCENARIO.json \
    > $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER/server.json &
    # store the server process PID. $! is the most recently spawned child pid
    SERVER_PID=$!

    # sleep for a small amount of time to allow the server to startup before the
    # client
    sleep 1

    # run the client. Port 4433 is the default for the server.
    echo "  running the client"
    SERVER_0=localhost:4433 ./$ARTIFACT_FOLDER/netbench-collector \
     ./$ARTIFACT_FOLDER/netbench-driver-$DRIVER-client \
     --scenario ./$NETBENCH_ARTIFACT_FOLDER/$SCENARIO.json \
     > $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER/client.json

    # cleanup server processes. The collector PID (which is the parent) is stored in
    # SERVER_PID. The collector forks the driver process. The following incantation
    # kills the child processes as well.
    echo "  killing the server"
    kill $(ps -o pid= --ppid $SERVER_PID)
}

# build all tools in the netbench workspace
cargo build --release

# generate the scenario files. This will generate .json files that can be found
# in the netbench/target/netbench directory. Config for all scenarios is done
# through this binary.
cargo run --manifest-path netbench-scenarios/Cargo.toml -- --request_response.response_size=8GiB --connect.connections 42

run_trial request_response s2n-quic
run_trial request_response s2n-tls

run_trial connect s2n-quic
run_trial connect s2n-tls

echo "generating the report"
./$ARTIFACT_FOLDER/netbench-cli report-tree $NETBENCH_ARTIFACT_FOLDER/results $NETBENCH_ARTIFACT_FOLDER/report

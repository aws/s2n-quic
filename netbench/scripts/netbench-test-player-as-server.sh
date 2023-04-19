#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# immediately exit if an error occurs.
set -e

ARTIFACT_FOLDER="target/release"
NETBENCH_ARTIFACT_FOLDER="target/netbench"

run_server() {
    SCENARIO=$1 # request-response, ping, ect...
    DRIVER=$2 # s2n-quic, s2n-tls, ect...
    COORD_CLIENT_0=$3
    echo "running the $SCENARIO scenario with $DRIVER at localhost:8080 against $COORD_CLIENT_0"

    # make a directory to hold the collected statistics
    mkdir -p $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER

    # run the server while collecting metrics; but wait on the client to start and stop the test
    echo "  running the server"
    ./$ARTIFACT_FOLDER/netbench-test-player \
    --run-as server \
    --remote-status-server $COORD_CLIENT_0 \
    ./$ARTIFACT_FOLDER/netbench-driver-$DRIVER-server \
    --scenario ./$NETBENCH_ARTIFACT_FOLDER/$SCENARIO.json \
    > $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER/server.json
}

run_server request_response s2n-quic $COORD_CLIENT_0

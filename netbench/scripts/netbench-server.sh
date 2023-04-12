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
    # e.g. request-response
    SCENARIO=$1
    # e.g. s2n-quic
    DRIVER=$2
    CLIENT_0=$3
    echo "running the $SCENARIO scenario with $DRIVER at localhost:8080 against $CLIENT_0"

    # make a directory to hold the collected statistics
    mkdir -p $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER

    # run the server while collecting metrics.
    echo "  running the server"
    ./$ARTIFACT_FOLDER/netbench-collector \
    --coordinate --as-server \
    --server-location "0.0.0.0:8080" \
    --client-location $CLIENT_0 \
    ./$ARTIFACT_FOLDER/netbench-driver-$DRIVER-server \
    --scenario ./$NETBENCH_ARTIFACT_FOLDER/$SCENARIO.json \
    > $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER/server.json
}

# build all tools in the netbench workspace
cargo build --release

# generate the scenario files. This will generate .json files that can be found
# in the netbench/target/netbench directory. Config for all scenarios is done
# through this binary.
# ./$ARTIFACT_FOLDER/netbench-scenarios --request_response.response_size=8GiB --connect.connections 42

run_server request_response s2n-quic $CLIENT_0

echo "generating the report"
./$ARTIFACT_FOLDER/netbench-cli report-tree $NETBENCH_ARTIFACT_FOLDER/results $NETBENCH_ARTIFACT_FOLDER/report

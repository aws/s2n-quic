#!/usr/bin/env bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

# immediately exit if an error occurs.
set -e

ARTIFACT_FOLDER="target/release"
NETBENCH_ARTIFACT_FOLDER="target/netbench"

# the run_trial function will run the request-response scenario
# with the driver passed in as the first argument
run_client() {
    # e.g. request-response
    SCENARIO=$1
    # e.g. s2n-quic
    DRIVER=$2
    SERVER_0=$3
    echo "running the $SCENARIO scenario with $DRIVER"

    mkdir -p $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER

    # run the client. Port 4433 is the default for the server.
    echo "  running the client"
    SERVER_0=$SERVER_0 ./$ARTIFACT_FOLDER/netbench-collector \
     --coordinate \
     --server-location $SERVER_0 \
     --client-location "0.0.0.0:8080" \
     ./$ARTIFACT_FOLDER/netbench-driver-$DRIVER-client \
     --scenario ./$NETBENCH_ARTIFACT_FOLDER/$SCENARIO.json \
     > $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER/client.json
}

# generate the scenario files. This will generate .json files that can be found
# in the netbench/target/netbench directory. Config for all scenarios is done
# through this binary.
# ./$ARTIFACT_FOLDER/netbench-scenarios --request_response.response_size=8GiB --connect.connections 42

run_client request_response s2n-quic $SERVER_0

echo "generating the report"
./$ARTIFACT_FOLDER/netbench-cli report-tree $NETBENCH_ARTIFACT_FOLDER/results $NETBENCH_ARTIFACT_FOLDER/report

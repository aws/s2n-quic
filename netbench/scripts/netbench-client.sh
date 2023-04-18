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
    COORD_SERVER_0=$3
    SERVER_0=$4
    echo "running the $SCENARIO scenario with $DRIVER"

    mkdir -p $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER

    # run the client. Port 4433 is the default for the server.
    echo "  running the client"
    SERVER_0=$SERVER_0 COORD_SERVER_0=$COORD_SERVER_0 ./$ARTIFACT_FOLDER/netbench-collector \
     --coordinate --run-as client \
     --server-location $COORD_SERVER_0 \
     --client-location "0.0.0.0:8080" \
     ./$ARTIFACT_FOLDER/netbench-driver-$DRIVER-client \
     --scenario ./$NETBENCH_ARTIFACT_FOLDER/$SCENARIO.json \
     > $NETBENCH_ARTIFACT_FOLDER/results/$SCENARIO/$DRIVER/client.json
}

run_client request_response s2n-quic $COORD_SERVER_0 $SERVER_0


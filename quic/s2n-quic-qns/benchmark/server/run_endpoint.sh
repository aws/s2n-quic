#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -e
set -o pipefail

# Set up the routing needed for the simulation
/setup.sh

# The following variables are available for use:
# - ROLE contains the role of this execution context, client or server
# - SERVER_PARAMS contains user-supplied command line parameters
# - CLIENT_PARAMS contains user-supplied command line parameters

LOG_DIR=/logs
LOG=$LOG_DIR/logs.txt

QNS_BIN="s2n-quic-qns"

if [ "$TEST_TYPE" == "MEASUREMENT" ] && [ -x "$(command -v s2n-quic-qns-release)" ]; then
    echo "using optimized build"
    QNS_BIN="s2n-quic-qns-release"
fi

CERT_ARGS=""

if [ -d "/certs" ]; then
    CERT_ARGS="--private-key /certs/priv.key --certificate /certs/cert.pem"
fi

$QNS_BIN perf \
    server \
    --connections 1 \
    --port 443 2>&1 | tee $LOG

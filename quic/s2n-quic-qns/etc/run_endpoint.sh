#!/bin/bash
#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

set -e
set -o pipefail

export RUST_LOG="debug"
export RUST_BACKTRACE="1"

# Set up the routing needed for the simulation
/setup.sh

# The following variables are available for use:
# - ROLE contains the role of this execution context, client or server
# - SERVER_PARAMS contains user-supplied command line parameters
# - CLIENT_PARAMS contains user-supplied command line parameters

LOG_DIR=/logs
LOG=$LOG_DIR/logs.txt

QNS_BIN="s2n-quic-qns"
QNS_MODE=${QNS_MODE:-interop}

if [ "$QNS_MODE" == "interop" ] && [ "$ROLE" == "server" ]; then
    SERVER_PARAMS+="--www-dir /www "
fi

# Disable GSO as it does not work with Docker
SERVER_PARAMS+="--max-gso-segments 1"

if [ "$TEST_TYPE" == "MEASUREMENT" ] && [ -x "$(command -v s2n-quic-qns-release)" ]; then
    echo "using optimized build"
    QNS_BIN="s2n-quic-qns-release"
    unset RUST_LOG
fi

CERT_ARGS=""

if [ -d "/certs" ]; then
    CERT_ARGS="--private-key /certs/priv.key --certificate /certs/cert.pem"
fi

if [ "$ROLE" == "client" ]; then
    # Wait for the simulator to start up.
    /wait-for-it.sh sim:57832 -s -t 30
    $QNS_BIN $QNS_MODE client \
        $CLIENT_PARAMS 2>&1 | tee $LOG
elif [ "$ROLE" == "server" ]; then
    $QNS_BIN $QNS_MODE server \
        $CERT_ARGS \
        $SERVER_PARAMS 2>&1 | tee $LOG
fi

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
TLS=${TLS:-s2n-tls}

# Disable GSO as it does not work with Docker
SERVER_PARAMS+=" --disable-gso"
CLIENT_PARAMS+=" --disable-gso"

if [ "$QNS_MODE" == "interop" ]; then
    if [ "$ROLE" == "server" ]; then
        SERVER_PARAMS+=" --www-dir /www"
    elif [ "$ROLE" == "client" ]; then
        CLIENT_PARAMS+=" --download-dir /downloads"
    fi
fi

SERVER_PARAMS+=" --tls $TLS"
CLIENT_PARAMS+=" --tls $TLS"

if [ "$TEST_TYPE" == "MEASUREMENT" ] && [ -x "$(command -v s2n-quic-qns-release)" ]; then
    echo "using optimized build"
    QNS_BIN="s2n-quic-qns-release"
    unset RUST_LOG
fi

# https://github.com/marten-seemann/quic-interop-runner/blob/1dedeab03da41716bdc3ab84243328ac2409f9fe/README.md#building-a-quic-endpoint
# The Interop Runner generates a key and a certificate chain and mounts it into /certs.
# The server needs to load its private key from priv.key, and the certificate chain from cert.pem.
if [ -d "/certs" ]; then
    if [ "$ROLE" == "server" ]; then
        SERVER_PARAMS+=" --private-key /certs/priv.key --certificate /certs/cert.pem"
    elif [ "$ROLE" == "client" ]; then
        CLIENT_PARAMS+=" --ca /certs/ca.pem"
    fi
fi

if [ "$ROLE" == "client" ]; then
    # Wait for the simulator to start up.
    /wait-for-it.sh sim:57832 -s -t 30
    $QNS_BIN $QNS_MODE client \
        $CLIENT_PARAMS \
        $REQUESTS > $LOG
elif [ "$ROLE" == "server" ]; then
    $QNS_BIN $QNS_MODE server \
        $SERVER_PARAMS > $LOG
fi


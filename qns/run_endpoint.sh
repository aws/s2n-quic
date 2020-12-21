#!/bin/bash
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

if [ "$TEST_TYPE" == "MEASUREMENT" ] && [ -x "$(command -v s2n-quic-qns-perf)" ]; then
    echo "using optimized build"
    QNS_BIN="s2n-quic-qns-perf"
fi

CERT_ARGS=""

if [ -d "/certs" ]; then
    openssl rsa -outform der -in /certs/priv.key -out /tmp/key.der
    openssl x509 -outform der -in /certs/cert.pem -out /tmp/cert.der
    openssl x509 -outform der -in /certs/ca.pem -out /tmp/ca.der

    # Default to the PEM format because our server can support single certs OR cert chains through PEM
    CERT_ARGS="--private-key /tmp/key.der --certificate /certs/cert.pem"
fi

if [ "$ROLE" == "client" ]; then
    # Wait for the simulator to start up.
    /wait-for-it.sh sim:57832 -s -t 30
    $QNS_BIN interop client \
        $CLIENT_PARAMS 2>&1 | tee $LOG
elif [ "$ROLE" == "server" ]; then
    $QNS_BIN interop server \
        --www-dir /www \
        $CERT_ARGS \
        $SERVER_PARAMS 2>&1 | tee $LOG
fi

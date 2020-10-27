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

if [ "$ROLE" == "client" ]; then
    # Wait for the simulator to start up.
    /wait-for-it.sh sim:57832 -s -t 30
    s2n-quic-qns interop client $CLIENT_PARAMS 2>&1 | tee $LOG
elif [ "$ROLE" == "server" ]; then
    s2n-quic-qns interop server \
      --www-dir /www \
      $SERVER_PARAMS 2>&1 | tee $LOG
fi

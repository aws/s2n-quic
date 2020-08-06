#!/bin/bash
set -e

# Set up the routing needed for the simulation
/setup.sh

# The following variables are available for use:
# - ROLE contains the role of this execution context, client or server
# - SERVER_PARAMS contains user-supplied command line parameters
# - CLIENT_PARAMS contains user-supplied command line parameters

if [ "$ROLE" == "client" ]; then
    s2n-quic-qns interop client $CLIENT_PARAMS
elif [ "$ROLE" == "server" ]; then
    s2n-quic-qns interop server $SERVER_PARAMS
fi

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

QNS_BIN="perf_client"

CERT_ARGS=""

if [ -d "/certs" ]; then
    CERT_ARGS="--private-key /certs/priv.key --certificate /certs/cert.pem"
fi

# Wait for the simulator to start up.
/wait-for-it.sh sim:57832 -s -t 30


if [ "$ROLE" == "server" ]; then
  perf_server \
  --listen "[::]:443" 2>&1 | tee $LOG
else
  perf_client  \
  --download-size "$DOWNLOAD_B" \
  --upload-size "$UPLOAD_B" \
  --insecure \
  --duration "$DURATION" \
  --json "/logs/perf_client.json" \
  server4:443  2>&1 | tee $LOG
fi
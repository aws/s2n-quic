#!/bin/bash

#
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
#

./setup.sh
./wait-for-it.sh sim:57832 -s -t 10

if [ "$ROLE" == "server" ]; then
  iperf3 -s
else
   ./wait-for-it.sh $SERVER:5201 -s -t 10
  iperf3 -c $SERVER -t $DURATION -i 1 -R -C $IPERF_CONGESTION --json > /logs/iperf.json
fi
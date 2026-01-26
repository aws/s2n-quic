#!/bin/bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0
echo "hellow world"
set -e
BUILDS=(
    "quic-attack"
)
usage() {
    echo "start_codebuild.sh <source_version> <repo>"
    echo "    example: start_codebuild.sh pr/1111"
    echo "    example: start_codebuild.sh 1234abcd"
    echo "    example: start_codebuild.sh test_branch lrstewart/s2n"
}

if [ "$#" -lt "1" ]; then
    usage
    exit 1
fi
SOURCE_VERSION=$1
REPO=${2:-aws/s2n-quic}

start_build() {
    NAME=$1
    REGION=${2:-"us-west-2"}  
    START_COMMAND="start-build"

        aws --region $REGION codebuild $START_COMMAND \
        --project-name $NAME \
        --source-location-override https://github.com/$REPO \
        --source-version $SOURCE_VERSION | jq -re "(.buildBatch.id // .build.id)"
}

for args in "${BUILDS[@]}"; do
    start_build $args
done
echo "All builds successfully started."


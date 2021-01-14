#/usr/bin/env bash

set -e

OUT=$(pwd)
TMP=$(mktemp -d)

VERSION=${1:-main}
REPO=${2:-https://github.com/awslabs/s2n.git}

#git clone $REPO $TMP
#cd $TMP
#git checkout $VERSION
#S2N=$TMP
S2N=$OUT/s2n

cargo run --manifest-path update/Cargo.toml


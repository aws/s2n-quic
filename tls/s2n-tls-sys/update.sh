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

bindgen \
  --use-core \
  --size_t-is-usize \
  --whitelist-type 's2n_.*' \
  --whitelist-function 's2n_.*' \
  --whitelist-var 's2n_.*' \
  --blacklist-type 'iovec' \
  --blacklist-type 'FILE' \
  --blacklist-type '_IO_.*' \
  --blacklist-type '__.*' \
  $OUT/s2n-sys.h \
  -o $OUT/src/vendored.rs \
  -- \
  -I$S2N/api \
  -D_S2N_QUIC_SUPPORT

# TODO replace '::std::os::raw' with '::libc'

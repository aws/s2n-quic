#!/usr/bin/env bash

set -e

N_COMMITS=${1:-50}

# get a list of N commit hashes
COMMITS=$(git log main -n $N_COMMITS --format="format:%H")
OUT_DIR=target/interop/results

# list of broken commits that should be ignored
SKIPLIST="
f1193adee0fb3e39d5d1142c5bc2bf01943edada
51d46666a3b3a9d9b0d5c58636dda3c4e043d256
d418b19c4b90952ca0f3e940e74b93fba0e724fa
"

mkdir -p $OUT_DIR

IFS=$'\n'
for commit in $COMMITS; do
  if [[ "$SKIPLIST" =~ (^|[[:space:]])$commit($|[[:space:]]) ]]; then
    echo "$commit in skiplist"
    continue
  fi

  echo $commit

  if [ ! -f $OUT_DIR/$commit.json ]; then
    curl --fail --silent -o $OUT_DIR/$commit.json \
      https://dnglbrstg7yg.cloudfront.net/$commit/interop/logs/latest/result.json || touch $OUT_DIR/$commit.json
  fi

  if [ -s $OUT_DIR/$commit.json ]; then
	  python3 .github/interop/check.py --required .github/interop/required.json $OUT_DIR/$commit.json
  else
    echo "    report could not be found; skipping"
  fi
done

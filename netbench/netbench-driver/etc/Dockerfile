FROM rust:latest as planner
WORKDIR app
RUN cargo install cargo-chef --version 0.1.23
COPY Cargo.toml /app
COPY common /app/common
COPY quic /app/quic
COPY netbench /app/netbench
RUN set -eux; \
  cargo chef prepare --recipe-path recipe.json; \
  cd netbench; \
  cargo chef prepare --recipe-path recipe.json;

FROM rust:latest as cacher
WORKDIR app
RUN cargo install cargo-chef --version 0.1.23
COPY --from=planner /app/recipe.json recipe.json
COPY --from=planner /app/netbench/recipe.json netbench/recipe.json

ARG release="true"
RUN set -eux; \
  export ARGS=""; \
  if [ "$release" = "true" ]; then \
    export ARGS="--release"; \
  fi; \
  cargo chef cook $ARGS --recipe-path recipe.json; \
  cd netbench; \
  cargo chef cook $ARGS --recipe-path recipe.json; \
  echo cooked;

FROM rust:latest AS builder
WORKDIR app

RUN set -eux; \
  apt-get update; \
  apt-get install -y cmake clang;

# copy sources
COPY Cargo.toml /app
COPY common /app/common
COPY quic /app/quic
COPY netbench /app/netbench

# Copy over the cached dependencies
COPY --from=cacher /app/target target
COPY --from=cacher /app/netbench/target netbench/target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

ARG release="true"

# build libs to improve caching between drivers
RUN set -eux; \
  export ARGS=""; \
  if [ "$release" = "true" ]; then \
    export ARGS="--release"; \
  fi; \
  cd netbench; \
  cargo build --lib $ARGS;

ARG DRIVER="s2n-quic"
ARG ENDPOINT="client"

RUN set -eux; \
  mkdir -p /app/bin; \
  export TARGET="netbench-driver-$DRIVER-$ENDPOINT"; \
  export COMMAND="netbench-collector /usr/bin/$TARGET"; \
  echo "#!/usr/bin/env bash\neval /usr/bin/$COMMAND \$@" > /app/bin/start; \
  cd netbench; \
  if [ "$release" = "true" ]; then \
    cargo build --bin $TARGET --release; \
    cp target/release/$TARGET /app/bin; \
    cargo build --bin netbench-collector --release; \
    cp target/release/netbench-collector /app/bin; \
  else \
    cargo build --bin $TARGET; \
    cp target/debug/$TARGET /app/bin; \
    cargo build --bin netbench-collector; \
    cp target/debug/netbench-collector /app/bin; \
  fi; \
  rm -rf target

FROM debian:latest

ENV RUST_BACKTRACE="1"

# copy driver
COPY --from=builder /app/bin /tmp/netbench
RUN set -eux; \
  chmod +x /tmp/netbench/*; \
  mv /tmp/netbench/* /usr/bin; \
  echo done

ENTRYPOINT ["/usr/bin/start"]

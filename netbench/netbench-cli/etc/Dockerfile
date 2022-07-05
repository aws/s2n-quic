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
  mkdir -p /app/bin; \
  cd netbench; \
  cargo build --lib $ARGS; \
  if [ "$release" = "true" ]; then \
    cargo build --bin netbench-cli --release; \
    cp target/release/netbench-cli /app/bin; \
  else \
    cargo build --bin netbench-cli; \
    cp target/debug/netbench-cli /app/bin; \
  fi; \
  rm -rf target; \
  echo "#!/usr/bin/env bash\naws s3 cp s3://\$S3_BUCKET/client.json ./client.json\naws s3 cp s3://\$S3_BUCKET/server.json ./server.json\neval /usr/bin/netbench-cli ./client.json ./server.json \$@ > report.json\naws s3 cp ./report.json s3://\$S3_BUCKET/" > /app/bin/start; 

FROM debian:latest

ENV RUST_BACKTRACE="1"

# copy driver
COPY --from=builder /app/bin /tmp/netbench
ENV DEBIAN_FRONTEND=noninteractive
RUN set -eux; \
  apt update && apt install -y dnsutils curl unzip sudo; \
  curl "https://awscli.amazonaws.com/awscli-exe-linux-aarch64.zip" -o "awscliv2.zip"; \
  unzip awscliv2.zip; \
  sudo ./aws/install; \
  chmod +x /tmp/netbench/*; \
  mv /tmp/netbench/* /usr/bin; \
  echo done

ENTRYPOINT ["/usr/bin/start"]

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
  # Bash script to retrieve client and server metrics from S3, and upload generated report to S3
  echo "#!/usr/bin/env bash\n \
  set -e\n \
  aws s3 cp s3://\$S3_BUCKET/client.json /tmp/client.json\n \
  aws s3 cp s3://\$S3_BUCKET/exportedlogs/\$EXPORT_TASK_ID/ /tmp/ --recursive\n \
  export DIR=\$(ls -d /tmp/*/) #get directory containing server logs\n \
  printenv\n \
  gzip -d \$(echo \$DIR)000000.gz #exported log file always named 000000.gz\n \
  sort -k1 \"\$(echo \$DIR)000000\" > \$(echo \$DIR)000000_sorted #sort server logs based on timestamp\n \
  echo \"\$(<\$(echo \$DIR)000000_sorted)\" |  awk -F'{' '{ st = index(\$0,\"{\");print \"{\" substr(\$0,st+1)}' > /tmp/server.json #remove timestamp from server logs\n \
  eval /usr/bin/netbench-cli report /tmp/client.json /tmp/server.json \$@ > /tmp/report.json\n \
  aws s3 cp /tmp/server.json s3://\$S3_BUCKET/\n \
  aws s3 cp /tmp/report.json s3://\$S3_BUCKET/" > /app/bin/start; 

FROM debian:latest

ENV RUST_BACKTRACE="1"

# copy driver
COPY --from=builder /app/bin /tmp/netbench
ENV DEBIAN_FRONTEND=noninteractive
RUN set -eux; \
  apt update && apt install -y dnsutils curl unzip sudo python3; \
  curl "https://awscli.amazonaws.com/awscli-exe-linux-aarch64.zip" -o "awscliv2.zip"; \
  unzip awscliv2.zip; \
  sudo ./aws/install; \
  chmod +x /tmp/netbench/*; \
  mv /tmp/netbench/* /usr/bin; \
  rm -r /tmp/netbench; \
  echo done

ENTRYPOINT ["/usr/bin/start"]

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

  echo "#!/usr/bin/env bash\n \
  set -e\n \
  # Wait until server logs have been successfully exported
  aws logs describe-export-tasks --task-id \$EXPORT_TASK_ID > task.json;\n \
  export DONE=\$(cat task.json | jq '.exportTasks[0].status.code');\n \
  export COMPLETED=\\\"COMPLETED\\\";\n \
  while [ \"\$DONE\" != \"\$COMPLETED\" ];\n \
  do\n \
  sleep 5;\n \
  aws logs describe-export-tasks --task-id \$EXPORT_TASK_ID > task.json;\n \
  export DONE=\$(cat task.json | jq '.exportTasks[0].status.code');\n \
  done;\n \
  aws s3 sync s3://\$S3_BUCKET/\$TIMESTAMP /tmp\n \
  # get directory containing server logs\n \
  export DIR=\$(ls -d /tmp/\$EXPORT_TASK_ID/*/)\n \
  printenv\n \
  # exported log file always named 000000.gz\n \
  gzip -d \$(echo \$DIR)000000.gz\n \
  # sort server logs based on timestamp\n \
  sort -k1 \"\$(echo \$DIR)000000\" > \$(echo \$DIR)000000_sorted\n \
  # remove timestamp from server logs\n \
  echo \"\$(<\$(echo \$DIR)000000_sorted)\" |  awk -F'{' '{ st = index(\$0,\"{\");print \"{\" substr(\$0,st+1)}' > /tmp/\$PROTOCOL-server.json\n \
  eval /usr/bin/netbench-cli report /tmp/\$PROTOCOL-client.json /tmp/\$PROTOCOL-server.json \$@ > /tmp/\$PROTOCOL-report.json\n \
  aws s3 sync /tmp s3://\$S3_BUCKET/\$TIMESTAMP" > /app/bin/start; 

FROM debian:latest

ENV RUST_BACKTRACE="1"

# copy driver
COPY --from=builder /app/bin /tmp/netbench
ENV DEBIAN_FRONTEND=noninteractive
RUN set -eux; \
  apt update && apt install -y dnsutils curl unzip sudo jq; \
  curl "https://awscli.amazonaws.com/awscli-exe-linux-aarch64.zip" -o "awscliv2.zip"; \
  unzip awscliv2.zip; \
  sudo ./aws/install; \
  aws configure set default.region us-west-2; \
  chmod +x /tmp/netbench/*; \
  mv /tmp/netbench/* /usr/bin; \
  rm -r /tmp/netbench; \
  echo done

ENTRYPOINT ["/usr/bin/start"]

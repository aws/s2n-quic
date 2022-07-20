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
    cargo build --bin netbench-collector --release; \
    cp target/release/netbench-collector /app/bin; \
  else \
    cargo build --bin netbench-collector; \
    cp target/debug/netbench-collector /app/bin; \
  fi; \
  rm -rf target

RUN set -eux; \
  cd netbench; \
  cargo build --bin netbench-scenarios; \
  ./target/debug/netbench-scenarios --request_response.count 10 --request_response.request_size 100KB --request_response.response_size 10MB; \
  cp target/netbench/request_response.json /app/bin; \
  rm -rf target;

ARG DRIVER="s2n-quic"
ARG ENDPOINT="client"

RUN set -eux; \
  export TARGET="netbench-driver-$DRIVER-$ENDPOINT"; \
  if [ "$ENDPOINT" = "server" ]; then \
    echo "#!/usr/bin/env bash\n \
    sudo mount -t debugfs debugfs /sys/kernel/debug;\n \
    eval /usr/bin/netbench-collector /usr/bin/$TARGET \$@" > /app/bin/start; \
  #  echo 1 > /proc/sys/kernel/sysrq;\n \
  #  echo x > /proc/sysrq-trigger;\n \
  #  dmesg;\n \
  else \
  #  echo "#!/usr/bin/env bash\n \
  #  sudo mount -t debugfs debugfs /sys/kernel/debug;\n \
  #  export SERVER_0=\$(dig +short \$DNS_ADDRESS):\$SERVER_PORT\n \
  #  printenv\n \
  #  eval /usr/bin/netbench-collector /usr/bin/$TARGET \$@ > client.json\n \
  #  aws s3 cp ./client.json s3://\$S3_BUCKET/\$TIMESTAMP/$DRIVER-client.json" > /app/bin/start; \
     echo "#!/usr/bin/env bash\n \
     sudo mount -t debugfs debugfs /sys/kernel/debug;\n \
     eval /usr/bin/netbench-collector /usr/bin/$TARGET \$@" > /app/bin/start; \
  fi; \
  cd netbench; \
  if [ "$release" = "true" ]; then \
    cargo build --bin $TARGET --release; \
    cp target/release/$TARGET /app/bin; \
  else \
    cargo build --bin $TARGET; \
    cp target/debug/$TARGET /app/bin; \
  fi; \
  rm -rf target;

FROM debian:latest

ENV RUST_BACKTRACE="1"
ARG ENDPOINT="client"

# copy driver
COPY --from=builder /app/bin /tmp/netbench
ENV DEBIAN_FRONTEND=noninteractive

RUN set -eux; \
  KERNEL_VERSION="${KERNEL_VERSION:-$(uname -r)}"; \
  kernel_version="$(echo "${KERNEL_VERSION}" | awk -vFS=- '{ print $1 }')"; \
  major_version="$(echo "${KERNEL_VERSION}" | awk -vFS=. '{ print $1 }')"; \
  apt-get update && apt-get install -y build-essential bc curl flex bison libelf-dev; \
  mkdir -p /usr/src/linux; \
  curl -sL "https://www.kernel.org/pub/linux/kernel/v${major_version}.x/linux-$kernel_version.tar.gz" \
    | tar --strip-components=1 -xzf - -C /usr/src/linux; \
  cd /usr/src/linux; \
  zcat /proc/config.gz > .config; \
  make ARCH=arm64 oldconfig; \
  make ARCH=arm64 prepare; \
  mkdir -p /lib/modules/$(uname -r); \
  ln -sf /usr/src/linux /lib/modules/$(uname -r)/source; \
  ln -sf /usr/src/linux /lib/modules/$(uname -r)/build; 

RUN set -eux; \
  apt-get update && apt-get install -y sudo; \
  sudo apt-get install clang libclang-dev bpftrace -y; \
  sudo apt-get install build-essential checkinstall bison \
    zlib1g-dev git cmake libcereal-dev libbpf-dev \
    flex g++ libelf-dev \
    zlib1g-dev \
    libfl-dev \
    systemtap-sdt-dev \
    binutils-dev \
    libcereal-dev \
    libpcap-dev \
    libgtest-dev \
    libgmock-dev \
    asciidoctor \
    wget -y; \
  wget -O - https://apt.llvm.org/llvm.sh; \
  #sudo apt-get install llvm-12-dev \
  #  llvm-12-runtime \
  #  libclang-12-dev \
  #  clang-12; \
  sudo apt-get install -y libbpfcc-dev; \
  sudo apt-get install linux-perf perf-tools-unstable linux-tools-5.10 -y; \
  git clone https://github.com/iovisor/bpftrace; \
  mkdir -p bpftrace/build; \
  cd bpftrace/build; \
  cmake -DCMAKE_BUILD_TYPE=Release ../; \
  make -j8; \
  sudo make install;

RUN set -eux; \
  cd; \
  if [ "$ENDPOINT" = "client" ]; then \
    apt-get update && apt-get install -y dnsutils curl unzip; \
    curl "https://awscli.amazonaws.com/awscli-exe-linux-aarch64.zip" -o "awscliv2.zip"; \
    unzip awscliv2.zip; \
    sudo ./aws/install; \
  fi; \
  chmod +x /tmp/netbench/*; \
  mv /tmp/netbench/* /usr/bin; \
  echo done

ENTRYPOINT ["/usr/bin/start"]

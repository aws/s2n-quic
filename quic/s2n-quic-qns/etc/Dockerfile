####################
# Base Build Image #
####################
FROM public.ecr.aws/amazonlinux/amazonlinux:2023-minimal AS rust-base

RUN set -eux; \
  dnf makecache --refresh; \
  dnf install -y gcc cmake; \
  dnf clean all; \
  rm -rf /var/cache/yum; \
  curl https://sh.rustup.rs -sSf | bash -s -- -y;

WORKDIR app

RUN set -eux; \
  source /root/.cargo/env; \
  cargo install cargo-chef --version 0.1.62 --locked; \
  cargo install cargo-auditable --version 0.6.1 --locked;

################
# Source image #
################
# create an image for all of the repo sources
#
FROM rust-base AS sources
COPY Cargo.toml /app
COPY common /app/common
COPY quic /app/quic
# Don't include testing crates
RUN set -eux; \
  sed -i '/xdp/d' quic/s2n-quic-platform/Cargo.toml; \
  sed -i '/xdp/d' quic/s2n-quic-qns/Cargo.toml; \
  sed -i '/xdp/d' quic/s2n-quic/Cargo.toml; \
  rm -rf quic/s2n-quic-bench quic/s2n-quic-events quic/s2n-quic-sim

#################
# Planner image #
#################
# create a planner image that forms the dependencies
FROM sources AS planner

RUN set -eux; \
  source /root/.cargo/env; \
  cargo chef prepare --recipe-path recipe.json;

#################
# Builder image #
#################
# create an image that builds the final crate
FROM sources AS builder

COPY --from=planner /app/recipe.json recipe.json

ENV RUSTFLAGS_REL="-C link-arg=-s -C panic=abort --cfg s2n_internal_dev"
ENV RUSTFLAGS_DEV="--cfg s2n_internal_dev"

ARG release="false"

RUN set -eux; \
  source /root/.cargo/env; \
  if [ "$release" = "true" ]; then \
    RUSTFLAGS="$RUSTFLAGS_REL" \
      cargo chef cook --release --recipe-path recipe.json; \
  else \
    RUSTFLAGS="$RUSTFLAGS_DEV" \
      cargo chef cook --recipe-path recipe.json; \
  fi;

# build runner
RUN set -eux; \
  source /root/.cargo/env; \
  if [ "$release" = "true" ]; then \
    RUSTFLAGS="$RUSTFLAGS_REL" \
      cargo auditable build -p s2n-quic-qns --bin s2n-quic-qns --release; \
    cp target/release/s2n-quic-qns .; \
  else \
    RUSTFLAGS="$RUSTFLAGS_DEV" \
      cargo auditable build -p s2n-quic-qns --bin s2n-quic-qns; \
    cp target/debug/s2n-quic-qns .; \
  fi; \
  rm -rf target

#######################
# Endpoint base image #
#######################
FROM public.ecr.aws/amazonlinux/amazonlinux:2023-minimal AS endpoint-base

ENV RUST_BACKTRACE="1"

ARG sim_hash="1f569016bcd9f7076fc9fd932c1415d28d3020f7"

RUN set -eux; \
  dnf makecache --refresh; \
  dnf install -y net-tools iputils ethtool iproute hostname; \
  dnf clean all; \
  rm -rf /var/cache/yum; \
  curl -o setup.sh https://raw.githubusercontent.com/marten-seemann/quic-network-simulator/${sim_hash}/endpoint/setup.sh; \
  chmod +x setup.sh; \
  curl -o wait-for-it.sh https://raw.githubusercontent.com/vishnubob/wait-for-it/master/wait-for-it.sh; \
  chmod +x wait-for-it.sh;

ARG tls
ENV TLS="${tls}"

ENTRYPOINT [ "/run_endpoint.sh" ]

###################
# Pre-built image #
###################
# Used when the qns binary is built outside of the docker context
FROM endpoint-base AS prebuilt

# copy entrypoint
COPY run_endpoint.sh .
RUN chmod +x run_endpoint.sh

# copy runner from the current context
COPY s2n-quic-qns-debug /usr/bin/s2n-quic-qns
COPY s2n-quic-qns-release /usr/bin/s2n-quic-qns-release

RUN set -eux; \
  chmod +x /usr/bin/s2n-quic-qns; \
  chmod +x /usr/bin/s2n-quic-qns-release; \
  ldd /usr/bin/s2n-quic-qns; \
  # ensure the binary works \
  s2n-quic-qns --help; \
  s2n-quic-qns-release --help; \
  echo done

###############
# iperf image #
###############
FROM endpoint-base AS iperf

RUN set -eux; \
  dnf makecache --refresh; \
  dnf install -y iperf3; \
  dnf clean all; \
  rm -rf /var/cache/yum;

COPY --from=sources /app/quic/s2n-quic-qns/benchmark/iperf/run.sh run_endpoint.sh
RUN chmod +x run_endpoint.sh

#################
# Default image #
#################
FROM endpoint-base AS default

# copy entrypoint
COPY --from=sources /app/quic/s2n-quic-qns/etc/run_endpoint.sh .
RUN chmod +x run_endpoint.sh

# copy runner
COPY --from=builder /app/s2n-quic-qns /usr/bin/s2n-quic-qns
RUN set -eux; \
  chmod +x /usr/bin/s2n-quic-qns; \
  ldd /usr/bin/s2n-quic-qns; \
  # ensure the binary works \
  s2n-quic-qns --help; \
  echo done


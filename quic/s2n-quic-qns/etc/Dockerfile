####################
# Base Build Image #
####################
FROM rust:latest AS rust-base

WORKDIR app

RUN set -eux; \
  apt-get update; \
  apt-get install -y cmake clang;

RUN cargo install cargo-chef --version 0.1.23

################
# Source image #
################
# create an image for all of the repo sources
#
FROM rust-base AS sources
COPY Cargo.toml /app
COPY common /app/common
COPY quic /app/quic
COPY tools/xdp /app/tools/xdp
# Don't include testing crates
RUN rm -rf quic/s2n-quic-bench quic/s2n-quic-events quic/s2n-quic-sim

#################
# Planner image #
#################
# create a planner image that forms the dependencies
FROM sources AS planner
RUN cargo chef prepare --recipe-path recipe.json

################
# Cacher image #
################
# create a cacher image that builds the dependencies
FROM rust-base AS cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --recipe-path recipe.json

#################
# Builder image #
#################
# create an image that builds the final crate
FROM sources AS builder

# Copy over the cached dependencies
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

# build runner
ARG release="false"
RUN set -eux; \
  if [ "$release" = "true" ]; then \
    RUSTFLAGS="-C link-arg=-s -C panic=abort --cfg s2n_internal_dev" \
      cargo build --bin s2n-quic-qns --release; \
    cp target/release/s2n-quic-qns .; \
  else \
    RUSTFLAGS="--cfg s2n_internal_dev" \
      cargo build --bin s2n-quic-qns; \
    cp target/debug/s2n-quic-qns .; \
  fi; \
  rm -rf target

#######################
# Endpoint base image #
#######################
FROM ubuntu:latest AS endpoint-base

ENV RUST_BACKTRACE="1"

ARG sim_hash="1f569016bcd9f7076fc9fd932c1415d28d3020f7"

RUN set -eux; \
  apt-get update; \
  apt-get install -y wget net-tools iputils-ping tcpdump ethtool iperf3 iproute2; \
  wget https://raw.githubusercontent.com/marten-seemann/quic-network-simulator/${sim_hash}/endpoint/setup.sh; \
  chmod +x setup.sh; \
  wget https://raw.githubusercontent.com/vishnubob/wait-for-it/master/wait-for-it.sh; \
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


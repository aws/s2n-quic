FROM martenseemann/quic-network-simulator-endpoint:latest

# install libcrypto
RUN set -eux; \
  apt-get update; \
  apt-get -y install libssl-dev; \
  rm -rf /var/lib/apt/lists/*; \
  apt-get clean; \
  rm -rf /tmp/*; \
  echo done;

# copy entrypoint
COPY run_endpoint.sh /
RUN chmod +x run_endpoint.sh

# copy runner
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

# help with debugging
ENV RUST_BACKTRACE=1

ARG tls
ENV TLS="${tls}"

# enable unstable features for testing
ENV S2N_UNSTABLE_CRYPTO_OPT_TX=100
ENV S2N_UNSTABLE_CRYPTO_OPT_RX=100

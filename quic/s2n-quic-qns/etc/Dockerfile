FROM martenseemann/quic-network-simulator-endpoint:latest

# copy entrypoint
COPY run_endpoint.sh /
RUN chmod +x run_endpoint.sh

# copy runner
COPY s2n-quic-qns /usr/bin/s2n-quic-qns
RUN set -eux; \
  chmod +x /usr/bin/s2n-quic-qns; \
  ldd /usr/bin/s2n-quic-qns; \
  # ensure the binary works \
  s2n-quic-qns --help; \
  echo done

# help with debugging
ENV RUST_BACKTRACE=1

# enable unstable crypto optimizations for testing
ENV S2N_UNSTABLE_CRYPTO_OPT_TX=100
ENV S2N_UNSTABLE_CRYPTO_OPT_RX=100

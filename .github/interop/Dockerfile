FROM martenseemann/quic-network-simulator-endpoint:latest

# copy entrypoint
COPY run_endpoint.sh /
RUN chmod +x run_endpoint.sh

# copy runner
COPY s2n-quic-qns-debug /usr/bin/s2n-quic-qns
COPY s2n-quic-qns-release /usr/bin/s2n-quic-qns-perf

RUN set -eux; \
  chmod +x /usr/bin/s2n-quic-qns; \
  chmod +x /usr/bin/s2n-quic-qns-perf; \
  ldd /usr/bin/s2n-quic-qns; \
  # ensure the binary works \
  s2n-quic-qns --help; \
  s2n-quic-qns-perf --help; \
  echo done

# help with debugging
ENV RUST_BACKTRACE=1

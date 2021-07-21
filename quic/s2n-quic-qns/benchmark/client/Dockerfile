FROM martenseemann/quic-network-simulator-endpoint:latest

# copy runner
COPY perf-client /usr/bin/perf-client
RUN set -eux; \
  chmod +x /usr/bin/perf-client; \
  ldd /usr/bin/perf-client; \
  # ensure the binary works \
  perf-client --help; \
  echo done

# copy entrypoint
COPY run_endpoint.sh /
RUN chmod +x run_endpoint.sh

# help with debugging
ENV RUST_BACKTRACE=1

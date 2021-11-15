FROM wesleyrosenblum/quic-network-simulator-endpoint:latest

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
COPY s2n-quic-qns /usr/bin/s2n-quic-qns
RUN set -eux; \
  chmod +x /usr/bin/s2n-quic-qns; \
  ldd /usr/bin/s2n-quic-qns; \
  # ensure the binary works \
  s2n-quic-qns --help; \
  echo done

# help with debugging
ENV RUST_BACKTRACE=1

FROM rust:latest AS builder

# remove debug symbols
ENV RUSTFLAGS="-C link-arg=-s -C panic=abort"

# copy sources
COPY Cargo.* /s2n-quic/
COPY common /s2n-quic/common
COPY quic /s2n-quic/quic
WORKDIR /s2n-quic

# build server
RUN cargo build --bin interop-server --release

FROM martenseemann/quic-network-simulator-endpoint:latest

COPY --from=builder /s2n-quic/target/release/interop-server /usr/bin/s2n-quic-qns

# copy run script and run it
COPY qns/run_endpoint.sh .
RUN chmod +x run_endpoint.sh
ENTRYPOINT [ "./run_endpoint.sh" ]

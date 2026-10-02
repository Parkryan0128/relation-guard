FROM rust:1.96.1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --release --locked -p authz-server -p authz-admin

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home authz
COPY --from=builder /build/target/release/authz-server /usr/local/bin/authz-server
COPY --from=builder /build/target/release/authz-admin /usr/local/bin/authz-admin
USER 10001:10001
ENV AUTHZ_BIND=0.0.0.0:8080 AUTHZ_OPS_BIND=0.0.0.0:9090
EXPOSE 8080
ENTRYPOINT ["authz-server"]

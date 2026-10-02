FROM rust:1.96.1-bookworm@sha256:a339861ae23e9abb272cea45dfafde21760d2ce6577a70f8a926153677902663 AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --release --locked -p authz-server -p authz-admin

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home authz
COPY --from=builder /build/target/release/authz-server /usr/local/bin/authz-server
COPY --from=builder /build/target/release/authz-admin /usr/local/bin/authz-admin
USER 10001:10001
ENV AUTHZ_BIND=0.0.0.0:8080 AUTHZ_OPS_BIND=0.0.0.0:9090
EXPOSE 8080
ENTRYPOINT ["authz-server"]

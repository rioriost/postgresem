FROM rust:1.85-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY contracts ./contracts
COPY schemas ./schemas
RUN cargo build --locked --release -p postgresem

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends gosu \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 postgresem
COPY --from=builder /workspace/target/release/postgresem /usr/local/bin/postgresem
USER postgresem
ENTRYPOINT ["postgresem"]

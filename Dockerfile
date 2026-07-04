# PDT — Pre-built binary runtime image
# Binary must be built locally before docker build:
#   CARGO_HOME=$HOME/.cargo cargo build --release
FROM docker.io/library/debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY target/release/pdt ./
COPY config-sample.toml ./config.toml
COPY policies/ ./policies/

ENV PDT_HOST=0.0.0.0
ENV PDT_PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["./pdt"]

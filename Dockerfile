# Multi-stage Rust build for rethink-cloud
FROM rust:1.85-bookworm AS build
WORKDIR /app

# Cache dependency builds
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# HTML assets are embedded at compile time via include_dir
COPY html ./html

RUN cargo build --release -p rethink-cloud -p rethink-setup -p rethink-tools \
    && strip target/release/rethink-cloud target/release/rethink-setup \
       target/release/packet-parser target/release/packet-sender

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates openssl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app \
    && useradd -r -g app app

COPY --from=build /app/target/release/rethink-cloud /usr/local/bin/rethink-cloud
COPY --from=build /app/target/release/rethink-setup /usr/local/bin/rethink-setup
COPY --from=build /app/target/release/packet-parser /usr/local/bin/packet-parser
COPY --from=build /app/target/release/packet-sender /usr/local/bin/packet-sender
COPY config.jsonc /app/config.json

RUN mkdir -p /app/data && chown -R app:app /app
USER app

EXPOSE 443 8883 1884 46030 47878 44401
CMD ["sh", "-c", "[ -f /app/data/config.json ] || cp /app/config.json /app/data/config.json; exec rethink-cloud /app/data/config.json"]

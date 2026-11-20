# syntax=docker/dockerfile:1
#
# LarvaPHP - a modern PHP application server written in Rust.
#
# Build:
#   docker build -t larva-php .
# Run (serving the current directory's PHP app):
#   docker run --rm -p 8000:8000 -v "$PWD:/app" larva-php
#
# The runtime image bundles PHP with the CGI SAPI (php-cgi), which LarvaPHP
# invokes per request.

# ----------------------------------------------------------------------------
# Build stage: compile the Rust binary against PHP headers.
# ----------------------------------------------------------------------------
FROM rust:1.83-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libclang-dev \
        php8.2-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

# Build only the main binary (release, optimized).
RUN cargo build --release -p larva-php --bin larva-php

# ----------------------------------------------------------------------------
# Runtime stage: minimal PHP CGI image + the larva-php binary.
# ----------------------------------------------------------------------------
FROM php:8.2-cgi-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        libsqlite3-0 \
        libzip4 \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/larva-php /usr/local/bin/larva-php

WORKDIR /app

# 8000 = HTTP, 9000 = optional CGI/metrics port.
EXPOSE 8000 9000

ENTRYPOINT ["larva-php"]
CMD ["--root", "/app", "--addr", "0.0.0.0:8000"]
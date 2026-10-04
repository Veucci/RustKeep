FROM rust:1-alpine AS server
RUN apk add --no-cache build-base
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY src src
RUN touch src/main.rs && cargo build --release

FROM node:22-bookworm-slim AS web
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates build-essential gzip \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain nightly --target wasm32-unknown-unknown \
    && curl -sL https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-gnu.tar.gz | tar xz -C /usr/local/cargo/bin
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml ./
RUN npm install --no-audit --no-fund
COPY web/Cargo.toml web/Cargo.lock web/rust-toolchain.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --target wasm32-unknown-unknown && rm -rf src
COPY web ./
RUN touch src/main.rs && trunk build --release && gzip -k9 dist/*.wasm dist/*.js dist/*.css

FROM alpine:3.22
RUN apk add --no-cache ca-certificates
COPY --from=server /app/target/release/rustkeep /usr/local/bin/rustkeep
COPY --from=web /web/dist /app/web/dist
WORKDIR /app
ENV DATA_DIR=/data STATIC_DIR=/app/web/dist PORT=8080
VOLUME /data
EXPOSE 8080
CMD ["rustkeep"]

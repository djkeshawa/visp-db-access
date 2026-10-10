FROM node:22-bookworm-slim AS web
WORKDIR /app/web
COPY web/package*.json ./
RUN npm ci
COPY docs/API.md docs/ARCHITECTURE.md /app/docs/
COPY web/ ./
RUN npm run build

FROM rust:1.93-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/
COPY --from=web /app/web/dist ./web/dist
RUN cargo build --locked --release -p vda-server

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/visp-db-access /usr/local/bin/visp-db-access
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/visp-db-access"]

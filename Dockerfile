FROM rust:1.85-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:3.21
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/simd /simd
EXPOSE 8080
VOLUME ["/data", "/config"]
ENTRYPOINT ["/simd"]
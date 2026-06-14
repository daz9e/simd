FROM rust:1.85-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release

FROM alpine:3.21 AS base
FROM scratch
ARG VERSION=0.1.0
LABEL org.opencontainers.image.title="simd"
LABEL org.opencontainers.image.description="A minimal, self-hosted markdown document viewer"
LABEL org.opencontainers.image.version="${VERSION}"
LABEL org.opencontainers.image.source="https://github.com/daz9e/simd"
LABEL org.opencontainers.image.licenses="MIT"
COPY --from=base /lib/ld-musl-*.so.1 /lib/
COPY --from=builder /app/target/release/simd /simd
EXPOSE 8080
VOLUME ["/data"]
ENTRYPOINT ["/simd"]

FROM rust:alpine as dependencies

RUN apk add --no-cache alpine-sdk cmake automake autoconf opus libtool openssl-dev libc6-compat

FROM dependencies as builder
WORKDIR app
COPY . .

RUN cargo build --release --bin groover

FROM alpine as runtime
WORKDIR app
COPY --from=builder /app/target/release/groover /app/groover

ENV CACHE_DIR=/data

ENTRYPOINT ["/app/groover"]
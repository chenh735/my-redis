FROM rust:1-slim-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin server --bin client --bin stress

FROM debian:bookworm-slim AS runtime

RUN useradd --create-home --uid 10001 appuser

WORKDIR /data

COPY --from=builder /app/target/release/server /usr/local/bin/server
COPY --from=builder /app/target/release/client /usr/local/bin/client
COPY --from=builder /app/target/release/stress /usr/local/bin/stress
COPY redis.conf /etc/my-redis/redis.conf

RUN chown -R appuser:appuser /data /etc/my-redis

USER appuser

EXPOSE 6379

CMD ["server", "--config", "/etc/my-redis/redis.conf", "--addr", "0.0.0.0"]

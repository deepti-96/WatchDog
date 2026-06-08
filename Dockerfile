FROM rust:1.87-slim AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/watchdog /usr/local/bin/watchdog

ENV WATCHDOG_EXPLAINER=local
ENV WATCHDOG_STORAGE=sqlite
ENV WATCHDOG_STATE_DIR=/data/watchdog
ENV WATCHDOG_DATABASE_URL=/data/watchdog/watchdog.sqlite
ENV WATCHDOG_DEMO_DEPLOY=v3.2.1
ENV WATCHDOG_DEMO_ENVIRONMENT=demo

EXPOSE 3000

CMD ["sh", "-c", "watchdog demo --state-dir \"${WATCHDOG_STATE_DIR}\" --deploy \"${WATCHDOG_DEMO_DEPLOY}\" --environment \"${WATCHDOG_DEMO_ENVIRONMENT}\" && watchdog serve --state-dir \"${WATCHDOG_STATE_DIR}\" --host 0.0.0.0 --port \"${PORT:-3000}\""]

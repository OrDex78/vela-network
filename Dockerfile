FROM rust:1.86 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin vela-node

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/vela-node .
COPY --from=builder /app/src/rpc/explorer.html src/rpc/explorer.html

ENV P2P_PORT=8001
ENV HTTP_PORT=9001
ENV VALIDATOR_INDEX=0
ENV BOOTSTRAP_ADDR=""
ENV DB_PATH="/data/vela_db"

EXPOSE ${P2P_PORT} ${HTTP_PORT}
CMD ./vela-node --validator-index ${VALIDATOR_INDEX:-0} --port ${P2P_PORT:-8001} --http-port ${HTTP_PORT:-9001}
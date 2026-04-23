FROM rust:1.86 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --bin vela-node

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/vela-node .
COPY --from=builder /app/src/rpc/explorer.html src/rpc/explorer.html
EXPOSE 8001 9001
CMD ["./vela-node", "--port", "8001"]
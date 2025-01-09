FROM docker.io/library/rust:1.83 AS builder

# Install libclang
RUN apt-get update && apt-get install -y \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory inside the container
WORKDIR /app/taiko_preconf_avs_node

COPY ../Node /app/taiko_preconf_avs_node
COPY ../p2pNode/p2pNetwork /app/p2pNode/p2pNetwork

RUN cargo build -p taiko_preconf_avs_node --release

FROM alpine:latest

COPY --from=builder /app/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node

ENTRYPOINT ["taiko_preconf_avs_node"]

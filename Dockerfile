FROM rust:1.87 AS builder

# Update CA certificates in builder stage
RUN apt-get update && apt-get install -y \
    libclang-dev \
    ca-certificates \
    && update-ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set the working directory inside the container
WORKDIR /app/taiko_preconf_avs_node

COPY . .

# Build taiko_preconf_avs_node
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/taiko_preconf_avs_node/target \
    cargo build -p taiko_preconf_avs_node --release \
    && mv /app/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /root

# Use small size system for final image
FROM gcr.io/distroless/cc

# Copy artifacts
COPY --from=builder /root/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /bin/sleep /bin/sleep

ENTRYPOINT ["taiko_preconf_avs_node"]
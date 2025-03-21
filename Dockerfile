# syntax = edrevo/dockerfile-plus

INCLUDE+ Dockerfile.build

# Set the working directory inside the container
WORKDIR /app/taiko_preconf_avs_node

COPY ../Node /app/taiko_preconf_avs_node
COPY ../p2pNode/p2pNetwork /app/p2pNode/p2pNetwork

# Build taiko_preconf_avs_node
RUN cargo build -p taiko_preconf_avs_node --release

# Use small size system for final image
FROM gcr.io/distroless/cc

# Copy artifacts
COPY --from=builder /app/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /bin/sleep /bin/sleep

ENTRYPOINT ["taiko_preconf_avs_node"]
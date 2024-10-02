# Build Stage
FROM docker.io/library/rust:1.80 AS builder

# Set the working directory inside the container
WORKDIR /usr/src/taiko_preconf_avs_node

# Copy the project files
COPY ../Node/src /usr/src/taiko_preconf_avs_node/src
COPY ../Node/Cargo.toml /usr/src/taiko_preconf_avs_node/Cargo.toml
COPY ../Node/Cargo.lock /usr/src/taiko_preconf_avs_node/Cargo.lock

# Copy the dependency directory
COPY ../p2pNode/p2pNetwork /usr/src/p2pNode/p2pNetwork

# Build the project in release mode
RUN cargo build -p taiko_preconf_avs_node --release

# Final Stage
FROM ubuntu:latest

# Install ca-certificates
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the build artifact from the builder stage
COPY --from=builder /usr/src/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node

# Ensure ca-certificates are up to date
RUN update-ca-certificates

# Expose the port that the server will run on (uncomment if needed)
# EXPOSE 9000

# Set the entrypoint
ENTRYPOINT ["taiko_preconf_avs_node"]
FROM rust:1.77 as builder

# Set the working directory inside the container
WORKDIR /usr/src/taiko_preconf_avs_node

# Copy the project files
COPY taiko_preconf_avs_node/ .

# Copy the dependency directory
COPY ../p2pNode/p2pNetwork /usr/src/p2pNode/p2pNetwork

# Build the project in release mode
RUN cargo build -p taiko_preconf_avs_node --release

# Use ubuntu as the base image
FROM ubuntu:latest

# Copy the build artifact from the builder stage
COPY --from=builder /usr/src/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node

# Expose the port that the server will run on
# EXPOSE 9000

# Run the binary
CMD ["taiko_preconf_avs_node"]

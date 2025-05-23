FROM rust:1.80 as builder

# Set the working directory inside the container
WORKDIR /usr/src/p2p-boot-node

# Copy the entire project into the container
COPY . .

# Build the project in release mode
RUN cargo build --release

# Use ubuntu as the base image
FROM ubuntu:latest

# Copy the build artifact from the builder stage
COPY --from=builder /usr/src/p2p-boot-node/target/release/p2p-boot-node /usr/local/bin/p2p-boot-node

# Expose the port that the server will run on
EXPOSE 9000

# Run the binary
# CMD in docker-compose
# CMD ["p2p-boot-node"]

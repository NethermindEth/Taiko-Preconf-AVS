FROM rust:1.77 as builder

# Set the working directory inside the container
WORKDIR /usr/src/p2p-node

# Copy the entire project into the container
COPY . .

# Build the project in release mode
RUN cargo build -p p2p-node --release

# Use ubuntu as the base image
FROM ubuntu:latest

# Copy the build artifact from the builder stage
COPY --from=builder /usr/src/p2p-node/target/release/p2p-node /usr/local/bin/p2p-node

# Expose the port that the server will run on
EXPOSE 9000

# Run the binary
CMD ["p2p-node"]

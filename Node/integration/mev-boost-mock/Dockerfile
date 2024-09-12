# Use the official Go image as a base image
FROM golang:1.22 as builder

# Set the Current Working Directory inside the container
WORKDIR /app

# Copy the Go source code into the container
COPY . .

# Download and install Go dependencies (if any)
# RUN go mod tidy

# Build the Go binary
RUN go build -o mev-boost-mock

# Use a minimal base image to run the compiled binary
FROM ubuntu:latest

# Set the working directory
WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/mev-boost-mock .

# Expose the port the server will run on
EXPOSE 8080

# Command to run the binary
CMD ["./mev-boost-mock"]

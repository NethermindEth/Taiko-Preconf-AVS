# syntax = edrevo/dockerfile-plus

INCLUDE+ Dockerfile.build

# Set the working directory inside the container
WORKDIR /app/taiko_preconf_avs_node

COPY ../Node /app/taiko_preconf_avs_node
COPY ../p2pNode/p2pNetwork /app/p2pNode/p2pNetwork
COPY ../tools/libsigner /app/libsigner

# Build the Go shared library (libsigner.so)
WORKDIR /app/libsigner
RUN cmake . && make

# Now, we need libsigner.so to be available to Rust build
WORKDIR /app/taiko_preconf_avs_node
RUN rm -f /app/taiko_preconf_avs_node/libsigner/libsigner.so
RUN rm -f /app/taiko_preconf_avs_node/libsigner/libsigner.h
RUN cp /app/libsigner/libsigner.so /app/taiko_preconf_avs_node/libsigner/libsigner.so
RUN cp /app/libsigner/libsigner.h /app/taiko_preconf_avs_node/libsigner/libsigner.h

# Build taiko_preconf_avs_node
RUN cargo build -p taiko_preconf_avs_node --release

# Use small size system for final image
FROM gcr.io/distroless/cc

# Copy artifacts
COPY --from=builder /app/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node
COPY --from=builder /app/taiko_preconf_avs_node/target/release/libsigner.so /usr/local/lib/libsigner.so
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /bin/sleep /bin/sleep

# Set LD_LIBRARY_PATH so libsigner.so is found
ENV LD_LIBRARY_PATH=/usr/local/lib

ENTRYPOINT ["taiko_preconf_avs_node"]
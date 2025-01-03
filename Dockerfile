FROM docker.io/library/rust:1.83 AS builder

WORKDIR /app/taiko_preconf_avs_node

COPY ../Node /app/taiko_preconf_avs_node
COPY ../p2pNode/p2pNetwork /app/p2pNode/p2pNetwork

RUN cargo build -p taiko_preconf_avs_node --release

FROM alpine:latest

# TODO:Install ca-certificates, fix for alpine
RUN apk add --no-cache ca-certificates

COPY --from=builder /app/taiko_preconf_avs_node/target/release/taiko_preconf_avs_node /usr/local/bin/taiko_preconf_avs_node

ENTRYPOINT ["taiko_preconf_avs_node"]

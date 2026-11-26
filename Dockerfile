# SPDX-License-Identifier: GPL-3.0-or-later
# Ash-Net: New Lawrence — Shared Multi-Stage Rust Workspace Dockerfile

FROM rust:slim-bookworm AS builder
WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY data ./data
COPY crates ./crates

RUN cargo build --release --workspace \
    && strip target/release/ash-auth-gateway \
             target/release/ash-game-node \
             target/release/ash-irc-bridge \
             target/release/ash-test-cli

# ----------------------------------------------------------------------------
# Target 1: pki-init (generiert lokale Ash-Net Root-CA & mTLS-Zertifikate)
# ----------------------------------------------------------------------------
FROM debian:bookworm-slim AS pki-init
COPY --from=builder /workspace/target/release/ash-auth-gateway /usr/local/bin/ash-pki-init
ENTRYPOINT ["/usr/local/bin/ash-pki-init", "--init-pki-only", "/certs"]

# ----------------------------------------------------------------------------
# Target 2: auth-gateway
# ----------------------------------------------------------------------------
FROM debian:bookworm-slim AS auth-gateway
RUN useradd -m -u 10001 ash
COPY --from=builder /workspace/target/release/ash-auth-gateway /usr/local/bin/ash-auth-gateway
USER ash
EXPOSE 4430/udp
ENTRYPOINT ["/usr/local/bin/ash-auth-gateway"]

# ----------------------------------------------------------------------------
# Target 3: game-node (Hub, Frontier-Mirror & skalierbare Raid-Nodes)
# ----------------------------------------------------------------------------
FROM debian:bookworm-slim AS game-node
RUN useradd -m -u 10001 ash
COPY --from=builder /workspace/target/release/ash-game-node /usr/local/bin/ash-game-node
USER ash
EXPOSE 4433/udp 5000/udp
ENTRYPOINT ["/usr/local/bin/ash-game-node"]

# ----------------------------------------------------------------------------
# Target 4: irc-bridge
# ----------------------------------------------------------------------------
FROM debian:bookworm-slim AS irc-bridge
RUN useradd -m -u 10001 ash
COPY --from=builder /workspace/target/release/ash-irc-bridge /usr/local/bin/ash-irc-bridge
USER ash
ENTRYPOINT ["/usr/local/bin/ash-irc-bridge"]

# ----------------------------------------------------------------------------
# Target 5: test-cli (Headless End-to-End Integration Runner)
# ----------------------------------------------------------------------------
FROM debian:bookworm-slim AS test-cli
RUN useradd -m -u 10001 ash
COPY --from=builder /workspace/target/release/ash-test-cli /usr/local/bin/ash-test-cli
USER ash
ENTRYPOINT ["/usr/local/bin/ash-test-cli"]

# TODO: set profile for release builds
# TODO: cache deps in separate layer
# TODO: use nix `dockerTools`?

FROM nixos/nix:latest AS builder

COPY . /tmp/build
WORKDIR /tmp/build

RUN nix \
    --extra-experimental-features "nix-command flakes" \
    --option filter-syscalls false \
    build

RUN mkdir /tmp/nix-store-closure
RUN cp -R $(nix-store -q$ result/) /tmp/nix-store-closure

FROM scratch

WORKDIR /app

COPY --from=builder /tmp/nix-store-closure /nix/store
COPY --from=builder /tmp/build/result /app

ENTRYPOINT [ "/app/bin/mev" ]

# FROM rust:1.67-bullseye AS chef
# RUN cargo install cargo-chef
# WORKDIR /app

# FROM chef AS planner
# COPY . .
# RUN cargo chef prepare --recipe-path recipe.json

# FROM chef AS builder
# COPY --from=planner /app/recipe.json recipe.json
# RUN cargo chef cook --release --recipe-path recipe.json
# COPY . .
# RUN cargo build --release

# FROM debian:bullseye-slim
# WORKDIR /app
# EXPOSE 18550
# COPY --from=builder /app/target/release/mev /usr/local/bin

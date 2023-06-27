FROM rust:1.70-bullseye AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
WORKDIR /app
EXPOSE 18550
COPY --from=builder /app/target/release/mev /usr/local/bin

ENTRYPOINT [ "/usr/local/bin/mev" ]

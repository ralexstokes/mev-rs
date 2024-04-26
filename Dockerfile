FROM rust:1.76-bullseye AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

ARG BUILD_PROFILE=release
ENV BUILD_PROFILE ${BUILD_PROFILE}

RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config

RUN cargo chef cook --profile ${BUILD_PROFILE} --recipe-path recipe.json

COPY . .
RUN cargo build --profile ${BUILD_PROFILE} --locked --bin mev

RUN cp /app/target/${BUILD_PROFILE}/mev /app/mev

FROM debian:bullseye-slim
WORKDIR /app

EXPOSE 18550
EXPOSE 28545
COPY --from=builder /app/target/release/mev /usr/local/bin

ENTRYPOINT [ "/usr/local/bin/mev" ]

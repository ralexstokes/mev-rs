FROM rust:1.81-bullseye AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

ARG BUILD_PROFILE=maxperf
ENV BUILD_PROFILE ${BUILD_PROFILE}
ARG FEATURES=""
ENV FEATURES ${FEATURES}

RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config

RUN cargo chef cook --profile ${BUILD_PROFILE} --features "$FEATURES" --recipe-path recipe.json

COPY . .
ARG RUSTFLAGS="-C target-cpu=native"
ENV RUSTFLAGS "$RUSTFLAGS"
RUN RUSTFLAGS="$RUSTFLAGS" cargo build --profile ${BUILD_PROFILE} --features "$FEATURES"  --locked --bin mev

RUN cp /app/target/${BUILD_PROFILE}/mev /app/mev

FROM debian:bullseye-slim
WORKDIR /app

EXPOSE 18550
EXPOSE 28545
COPY --from=builder /app/mev /usr/local/bin

ENTRYPOINT [ "/usr/local/bin/mev" ]

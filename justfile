validate-example-config:
    cargo run config example.config.toml

docker-build:
    docker build -t ralexstokes/mev-rs .
docker-push:
    docker push ralexstokes/mev-rs
docker-update: docker-build docker-push

test:
    cargo test --all
fmt:
    cargo +nightly-2023-10-29 fmt --all
lint: fmt validate-example-config
    cargo +nightly-2023-10-29 clippy # --all-targets --all-features --all
build:
    cargo build --all-targets --all-features --all
run-ci: lint build test

build-book:
    mdbook build
serve-book:
    mdbook serve --open

validate-example-config:
    cargo run config example.config.toml

docker-build tag="latest":
    docker buildx build -t ralexstokes/mev-rs:{{tag}} . --platform linux/amd64
docker-push:
    docker push ralexstokes/mev-rs
docker-update: docker-build docker-push

test:
    cargo test --all
fmt:
    cargo +nightly fmt --all
lint: fmt validate-example-config
    cargo +nightly clippy --all-targets --all-features --all
build:
    cargo build --all-targets --all-features --all
run-ci: lint build test

build-book:
    mdbook build
serve-book:
    mdbook serve --open

validate-example-config:
    cargo run config example.config.toml

build-docker:
    docker build -t ralexstokes/mev-rs .
push-docker:
    docker push ralexstokes/mev-rs
update-docker-hub: build-docker push-docker

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

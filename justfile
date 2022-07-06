test:
    cargo test
run-integration-tests:
    cargo test --test '*'
fmt:
    cargo fmt
lint: fmt validate-example-config
    cargo clippy
validate-example-config:
    cargo run config example.config.toml
build:
    cargo build
run-ci: lint build test
docker-build:
    docker build -t ralexstokes/mev-rs .
docker-push:
    docker push ralexstokes/mev-rs
docker-update: docker-build docker-push

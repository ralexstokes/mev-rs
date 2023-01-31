run-integration-tests:
    cargo test --all --test '*'
validate-example-config:
    cargo run config example.config.toml

docker-build:
    docker build -t ralexstokes/mev-rs .
docker-push:
    docker push ralexstokes/mev-rs
docker-update: docker-build docker-push

test:
    # Partitions much heavier "integration tests" to a separate command
    cargo test --all --lib
fmt:
    cargo +nightly fmt --all
lint: fmt validate-example-config
    cargo +nightly clippy --all-targets --all-features --all
build:
    cargo build --all-targets --all-features --all
run-ci: lint build test

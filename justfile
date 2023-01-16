run-integration-tests:
    cargo test --test '*'
validate-example-config:
    cargo run config example.config.toml

docker-build:
    docker build -t ralexstokes/mev-rs .
docker-push:
    docker push ralexstokes/mev-rs
docker-update: docker-build docker-push

test:
    # Partitions much heavier "integration tests" to a separate command
    cargo test --lib
fmt:
    cargo fmt
lint: fmt validate-example-config
    cargo clippy --all-targets --all-features
build:
    cargo build --all-features
run-ci: lint build test

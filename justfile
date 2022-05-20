test:
    cargo test
fmt:
    cargo fmt
lint: fmt
    cargo clippy
build:
    cargo build
run-ci: lint build test
docker-build:
    docker build -t mev-rs .
docker-push:
    docker push ralexstokes/mev-rs
docker-update: docker-build docker-push
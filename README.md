# mev-rs

[![build](https://github.com/ralexstokes/mev-rs/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/ralexstokes/mev-rs/actions/workflows/rust.yml)

(experimental) utilities for block space.

# About

`mev-rs` bundles a series of utilities for interacting with an external builder network.

# 🚧 WARNING 🚧

This project is currently experimental and subject to frequent changes as we are still working on stabilizing the builder specs.

It has not been audited for security purposes and should not be used in production yet.

# Status

Implements `v0.0.0` of the [`builder-specs`](https://github.com/ethereum/builder-specs)

# Requirements

You need a recent Rust toolchain to get started.

If you don't have one already, check out: https://www.rust-lang.org/tools/install

Once you do that, you can just use `cargo` as specified below.

# How to use

`mev-rs` builds a command line utility with a series of subcommands.

## Subcommands

### 🚀 `boost`

runs a builder multiplexer, a gateway for validators to connect to a network of block builders.

the binary takes a path to a configuration file as a command line argument.

this argument can also be provided as an environment variable `CONFIG_FILE=$FILE`.

an example configuration file is provided at `example.config.toml`.

#### how to run

to run the multiplexer, you have the following options:

##### run natively

just run with `cargo`:

`$ cargo run boost example.config.toml`

##### run with docker

you can build the image defined in the `Dockerfile`:

`$ docker build -t mev-rs .`

and then run as usual.

to supply the config, mount your local file into the container (e.g. at `/config.toml`) and either use the environment
variable setting or provide the path in the container as a trailing arguemnt:

`$ docker <other options> --env CONFIG_FILE=/path/to/config.toml mev-rs boost`

or

`$ docker <other options> mev-rs boost /config.toml`

### 🏗 `relay`

runs a builder relay.

the configuration works the same way as with `boost`.

there is an option to run a "mock" relay:

`$ cargo run relay mock example.config.toml`

# Testing

`cargo test` to run the tests.

# Contributing

Contributions are welcome!

Please submit a PR to this repo to contribute.

You can run the CI locally to see what is required to pass.

For convenience, the flow is defined in the top-level `justfile`.

To run, `just run-ci`.
# mev-rs

(experimental) utilities for block space.

# About

`mev-rs` bundles a series of utilities for interacting with an external builder network.

## Subcommands

# `boost`

runs a builder multiplexer.

the binary takes a path to a configuration file as a command line argument.

this argument can also be provided as an environment variable `CONFIG_FILE=$FILE`.

an example configuration file is provided at `example.config.toml`.

## how to run

to run the multiplexer, you have the following options:

### run natively

just run with `cargo`:

`$ cargo run boost example.config.toml`

### run with docker

you can build the image defined in the `Dockerfile`:

`$ docker build -t mev-rs .`

and then run as usual.

to supply the config, mount your local file into the container (e.g. at `/config.toml`) and either use the environment
variable setting or provide the path in the container as a trailing arguemnt:

`$ docker <other options> --env CONFIG_FILE=/path/to/config.toml mev-rs boost`

or

`$ docker <other options> mev-rs boost /config.toml`
# mev-rs

(experimental) tooling for blockspace.

# `mev-boost-rs`

this repo currently provides a builder multiplexer in `mev-boost-rs`.

the binary takes a path to a configuration file as a command line argument.

this argument can also be provided as an environment variable `CONFIG_FILE=$FILE`.

an example configuration file is provided at `example.config.toml`.

## how to run

to run the multiplexer, you have the following options:

### run natively

just run with `cargo`:

`$ cargo run -- --config-file config.toml`

### run with docker

you can build the image defined in the `Dockerfile`:

`$ docker build -t mev-boost-rs .`

and then run as usual.

to supply the config, mount your local file into the container and use the environment variable setting.

you can supply the absolute file path of the mounted config but using an environment variable is more convenient.

`$ docker <other options> --env CONFIG_FILE=/path/to/config.toml mev-boost-rs`
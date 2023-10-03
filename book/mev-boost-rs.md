# `mev-boost-rs`

This repository provides a "boost" implementation that serves the [`builder-specs` APIs]
connecting validators to an external block builder network.

> Warning: this software is extremely experimental. You should not use in any production capacity.

## Installation

### Build from source

The best way to use this right now is to build from source.

## Run `mev-boost-rs`

You'll need to construct a configuration file like the `example.config.toml` found
at the root of this directory.

For example, this configuration

```toml
[boost]
host = "0.0.0.0"
port = 18550
relays = [
    "https://0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a@boost-relay-sepolia.flashbots.net",
]
```

instructs `mev-boost-rs` to run on localhost on port `18550` and only use the Flashbots relay running on the `sepolia` testnet.

Then, to run for `sepolia`:
```bash
mev --network sepolia boost example.config.toml
```

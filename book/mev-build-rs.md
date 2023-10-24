# `mev-build-rs`

This repository provides a basic builder which can submit blocks to [`mev-boost` relays](https://boost.flashbots.net) using the [Relay APIs](https://flashbots.github.io/relay-specs/).

The builder is built as an extension to the [`reth`](https://github.com/paradigmxyz/reth) execution layer (EL) client and also requires a consensus layer (CL) client to run.

The default builder simply uses `reth`'s local mempool when sourcing transactions.

## Installation

### Build from source

The best way to run the builder right now is to build this project from source.

#### Prerequisites

Follow the steps [here under `Dependencies`](https://paradigmxyz.github.io/reth/installation/source.html#dependencies).

#### Build `mev-rs`

You can install the `mev-rs` binary, named `mev`, with the following steps:

```sh
git clone https://github.com/ralexstokes/mev-rs
cd mev-rs
cargo install --locked --path bin/mev
```

> The builder has been verified as of this commit `08973a298268a3ad5f5d2c247b69b47dbb7bf97f`. No guarantees about other states of the repository currently.

The `cargo install` command should place the `mev` binary under the default `.cargo/bin` folder which is also in your `PATH` following the suggested Rust installation process.

## Run the builder

### Configuration

To run the `mev` builder, first you should make the appropriate configuration. You can make a local copy of `example.config.toml` to get started.

First, you will need to construct a JWT secret for use in the Engine API. You can refer to [these instructions from the Lighthouse guide](https://lighthouse-book.sigmaprime.io/run_a_node.html#step-1-create-a-jwt-secret-file) to see how to do this.

Ensure the `network` key in the TOML matches the target network you wish to run the builder on. This network applies to any of the `mev-rs` tools
that consume this configuration. The remainder of this document (including examples below) will assume we are building for the `sepolia` network.

To configure the builder specifically, you can edit the fields under the `[builder]` key of the TOML.

Fields you should change:

* `execution_mnemonic`: update to a seed phrase of an ethereum wallet you control.
  This wallet will be used to author payment transactions to the proposer and also is used as the source of funds for any subsidy value you wish to add to the block.
  You can select a particular index (following BIP-39) by terminating the seed phrase with a `:N` and integer index `N`. Otherwise the builder will just use the first index from the key tree.
* `subsidy_gwei`: set this value to 0 if your execution layer address has no ETH in it; otherwise, the blocks will be invalid.
* `jwt_secret_path`: this path points to the JWT secret file created previously and is specific to your deployment.

### Launch

Once the configuration looks good, you can run the builder as follows alongside `lighthouse`. If you are running from a fresh install or have fallen far enough behind
the tip of the chain, the CL and EL nodes will sync. To expedite syncing times, use of checkpoint sync is recommended. You can see more info in [this guide from the Lighthouse book](https://lighthouse-book.sigmaprime.io/run_a_node.html).

> Repository has only been tested on the **Sepolia** network and there is no guarantee the builder works on other networks.

> Repository has only been tested with **Lighthouse** and there is no guarantee other CLs will work.

1. Run `mev` with config file `config.toml`:
  ```sh
  mev build config.toml
  ```

2. Run `lighthouse`:
  ```sh
  lighthouse --network sepolia \
    bn \
    --http \
    --execution-endpoint http://localhost:8551 \
    --execution-jwt $JWT_SECRET_FILE_PATH \
    --disable-deposit-contract-sync \
    --checkpoint-sync-url $CHECKPOINT_SYNC_PROVIDER
    --always-prepare-payload
    --suggested-fee-recipient $FEE_RECIPIENT
  ```

> NOTE: the builder will not be active until the local CL and EL are fully synced.

### Additional notes

* The `--suggested-fee-recipient` for `lighthouse` is ultimately not used, but currently required to run the node. Any valid address should do and it should not affect the builder.
* If you are seeing slow or lagging operation, you can try to adjust the preparation lookahead with the `--prepare-payload-lookahead` option on `lighthouse`.
* The builder has been tested on an AWS EC2 instance of `t3.xlarge` variety with a `512Gb` disk.
* You can control the logging level of `reth` and `mev` with the `RUST_LOG` environment variable.
  For example, to silence the `reth` logs, you can run `mev` like `RUST_LOG=mev=info mev build config.toml`

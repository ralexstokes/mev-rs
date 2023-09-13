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
cargo install --locked
```

> The builder has been verified as of this commit `bf3d41f026e9728233dd3e1c40e75c49b9ae00b3`. No guarantees about other states of the repository currently.

The `cargo install` command should place the `mev` binary under the default `.cargo/bin` folder which is also in your `PATH` following the suggested Rust installation process.

## Run the builder

Once installed, we are ready to run the builder.

### Syncing nodes

Before we can run the builder, we need to sync a CL and EL node pair on our target network.

The remainder of this document will assume we are building for the `sepolia` network.

> Repository has only been tested on the **Sepolia** network and there is no guarantee the builder works on other networks.

The builder requires a synced CL client like [Lighthouse](https://github.com/sigp/lighthouse/).
You can find instructions on [how to sync a `lighthouse` node here](https://lighthouse-book.sigmaprime.io).

> Repository has only been tested with **Lighthouse** and there is no guarantee other CLs will work.

Doing the initial/bulk sync from the `mev` builder should be possible, but has not been tested.

The recommended approach will be to run `reth` (ideally built from source at the same commit pinned in this repo) for the target network alongside the CL until the pair has reached the head of the chain.

Example commands utilizing [checkpoint sync following the Lighthouse book](https://lighthouse-book.sigmaprime.io/run_a_node.html) to do this:

1. Make the JWT secret (refer to the Lighthouse guide for more info).

2. [recommended] Obtain a checkpoint sync URL if you wish to use this sync mode.

3. Run `reth`:
  ```sh
  reth --chain sepolia \
    node \
    --http \
    --authrpc.jwtsecret $JWT_SECRET_FILE_PATH
  ````

4. Run `lighthouse`:
  ```sh
  lighthouse --network sepolia \
    bn \
    --http \
    --execution-endpoint http://localhost:8551 \
    --execution-jwt $JWT_SECRET_FILE_PATH \
    --disable-deposit-contract-sync \
    --checkpoint-sync-url $CHECKPOINT_SYNC_PROVIDER
  ```

The pair should start syncing. Once the pair of nodes is fully synced you can stop `reth` and run the `mev` builder in its place.

> You should be able to skip this step [Syncing nodes](#syncing-nodes) and just proceed directly to running the CL and `mev` builder in the [next step](#run-the-builder-on-a-synced-chain), as the builder should also sync if needed.
> But note:
> 1) running the builder without having a synced database already has not been tested
> 2) the builder will wait anyway until the head of the chain has been synced

### Run the builder on a synced chain

To run the `mev` builder, first you should make the appropriate configuration. You can make a local copy of `example.config.toml` to get started.

To configure the builder, you can edit the fields under the `[builder]` key of the TOML.

Fields you should change:

* `execution_mnemonic`: update to a seed phrase of an ethereum wallet you control.
  This wallet will be used to author payment transactions to the proposer and also is used as the source of funds for any subsidy value you wish to add to the block.
  You can select a particular index (following BIP-39) by terminating the seed phrase with a `:N` and integer index `N`. Otherwise the builder will just use the first index from the key tree.
* `subsidy_gwei`: set this value to 0 if your execution layer address has no ETH in it; otherwise, the blocks will be invalid.
* `jwt_secret_path`: ensure this value matches the one used previously when doing the initial sync.

Once the configuration looks good, you can run the builder as follows alongside `lighthouse`.

> `lighthouse` has some additional configuration from above to ensure the builder always receives head updates from the chain.

1. Run `mev` with config file `config.toml`:
  ```sh
  mev --network sepolia build config.toml
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

### Additional notes

* The `--suggested-fee-recipient` for `lighthouse` is ultimately not used, but currently required to run the node. Any valid address should do and it should not affect the builder.
* If you are seeing slow or lagging operation, you can try to adjust the preparation lookahead with the `--prepare-payload-lookahead` option on `lighthouse`.
* The builder has been tested on an AWS EC2 instance of `t3.xlarge` variety with a `512Gb` disk.

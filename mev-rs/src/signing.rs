use ethereum_consensus::{
    builder::compute_builder_domain,
    domains::DomainType,
    phase0::mainnet::compute_domain,
    primitives::{BlsSignature, Root, Slot},
    signing::sign_with_domain,
    ssz::prelude::Merkleized,
    state_transition::Context,
    Error, Fork,
};
pub use ethereum_consensus::{
    crypto::SecretKey,
    signing::{compute_signing_root, verify_signature},
};

pub fn compute_consensus_signing_root<T: Merkleized>(
    data: &mut T,
    slot: Slot,
    genesis_validators_root: &Root,
    context: &Context,
) -> Result<Root, Error> {
    let fork_version = match context.fork_for(slot) {
        Fork::Bellatrix => context.bellatrix_fork_version,
        Fork::Capella => context.capella_fork_version,
        Fork::Deneb => context.deneb_fork_version,
        _ => unimplemented!("fork not supported"),
    };
    let domain = compute_domain(
        DomainType::BeaconProposer,
        Some(fork_version),
        Some(*genesis_validators_root),
        context,
    )?;
    compute_signing_root(data, domain)
}

pub fn sign_builder_message<T: Merkleized>(
    message: &mut T,
    signing_key: &SecretKey,
    context: &Context,
) -> Result<BlsSignature, Error> {
    let domain = compute_builder_domain(context)?;
    sign_with_domain(message, signing_key, domain)
}

pub fn compute_builder_signing_root<T: Merkleized>(
    data: &mut T,
    context: &Context,
) -> Result<Root, Error> {
    let domain = compute_builder_domain(context)?;
    compute_signing_root(data, domain)
}

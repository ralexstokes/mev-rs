use ethereum_consensus::{
    builder::compute_builder_domain,
    crypto,
    domains::DomainType,
    phase0::compute_domain,
    primitives::{BlsPublicKey, BlsSignature, Domain, Root, Slot},
    signing::{compute_signing_root, sign_with_domain},
    ssz::prelude::HashTreeRoot,
    state_transition::Context,
    Error,
};
pub use ethereum_consensus::{crypto::SecretKey, signing::verify_signed_data};

pub fn compute_consensus_domain(
    slot: Slot,
    genesis_validators_root: &Root,
    context: &Context,
) -> Result<Domain, Error> {
    let fork = context.fork_for(slot);
    let fork_version = context.fork_version_for(fork);
    compute_domain(
        DomainType::BeaconProposer,
        Some(fork_version),
        Some(*genesis_validators_root),
        context,
    )
}

pub fn sign_builder_message<T: HashTreeRoot>(
    message: &T,
    signing_key: &SecretKey,
    context: &Context,
) -> Result<BlsSignature, Error> {
    let domain = compute_builder_domain(context)?;
    sign_with_domain(message, signing_key, domain)
}

pub fn verify_signed_builder_data<T: HashTreeRoot>(
    data: &T,
    public_key: &BlsPublicKey,
    signature: &BlsSignature,
    context: &Context,
) -> Result<(), Error> {
    let domain = compute_builder_domain(context)?;
    let signing_root = compute_signing_root(data, domain)?;
    crypto::verify_signature(public_key, signing_root.as_ref(), signature).map_err(Into::into)
}

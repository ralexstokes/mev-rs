pub use ethereum_consensus::signing::{compute_signing_root, verify_signature};
use ethereum_consensus::{
    builder::compute_builder_domain,
    crypto::SecretKey,
    domains::DomainType,
    phase0::mainnet::compute_domain,
    primitives::{BlsPublicKey, BlsSignature, Root, Slot},
    signing::{sign_with_domain, verify_signed_data},
    ssz::prelude::SimpleSerialize,
    state_transition::{Context, Error},
    Fork,
};

pub fn verify_signed_consensus_message<T: SimpleSerialize>(
    message: &mut T,
    signature: &BlsSignature,
    public_key: &BlsPublicKey,
    context: &Context,
    slot_hint: Option<Slot>,
    root_hint: Option<Root>,
) -> Result<(), Error> {
    let fork_version = slot_hint.map(|slot| match context.fork_for(slot) {
        Fork::Bellatrix => context.bellatrix_fork_version,
        Fork::Capella => context.capella_fork_version,
        _ => unimplemented!(),
    });
    let domain =
        compute_domain(DomainType::BeaconProposer, fork_version, root_hint, context).unwrap();
    verify_signed_data(message, signature, public_key, domain)?;
    Ok(())
}

pub fn sign_builder_message<T: SimpleSerialize>(
    message: &mut T,
    signing_key: &SecretKey,
    context: &Context,
) -> Result<BlsSignature, Error> {
    let domain = compute_builder_domain(context)?;
    let signature = sign_with_domain(message, signing_key, domain)?;
    Ok(signature)
}

pub fn compute_builder_signing_root<T: SimpleSerialize>(
    data: &mut T,
    context: &Context,
) -> Result<Root, Error> {
    let domain = compute_builder_domain(context)?;
    compute_signing_root(data, domain)
}

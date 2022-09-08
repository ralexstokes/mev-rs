use ethereum_consensus::{
    builder::compute_builder_domain,
    crypto::SecretKey,
    domains::DomainType,
    phase0::mainnet::compute_domain,
    primitives::{BlsPublicKey, BlsSignature},
    signing::{sign_with_domain, verify_signed_data},
    state_transition::{Context, Error},
};
use ssz_rs::prelude::SimpleSerialize;

pub fn verify_signed_consensus_message<T: SimpleSerialize>(
    message: &mut T,
    signature: &BlsSignature,
    public_key: &BlsPublicKey,
    context: &Context,
) -> Result<(), Error> {
    // TODO use real values...
    let domain = compute_domain(DomainType::BeaconProposer, None, None, context).unwrap();
    verify_signed_data(message, signature, public_key, domain)?;
    Ok(())
}

pub fn verify_signed_builder_message<T: SimpleSerialize>(
    message: &mut T,
    signature: &BlsSignature,
    public_key: &BlsPublicKey,
    context: &Context,
) -> Result<(), Error> {
    let domain = compute_builder_domain(context)?;
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

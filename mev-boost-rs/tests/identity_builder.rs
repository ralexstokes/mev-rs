use async_trait::async_trait;
use ethereum_consensus::{
    builder::{SignedValidatorRegistration, ValidatorRegistration},
    crypto::SecretKey,
    primitives::{BlsPublicKey, Slot, U256},
    state_transition::Context,
    Fork,
};
use mev_rs::{
    blinded_block_provider::BlindedBlockProvider,
    signing::sign_builder_message,
    types::{
        builder_bid, AuctionContents, AuctionRequest, BuilderBid, ExecutionPayload,
        ExecutionPayloadHeader, SignedBlindedBeaconBlock, SignedBuilderBid,
    },
    Error,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[cfg(not(feature = "minimal-preset"))]
use ethereum_consensus::{bellatrix::mainnet as bellatrix, capella::mainnet as capella};
#[cfg(feature = "minimal-preset")]
use ethereum_consensus::{bellatrix::minimal as bellatrix, capella::minimal as capella};

#[derive(Clone)]
pub struct IdentityBuilder {
    signing_key: SecretKey,
    public_key: BlsPublicKey,
    context: Arc<Context>,
    bids: Arc<Mutex<HashMap<Slot, ExecutionPayload>>>,
    registrations: Arc<Mutex<HashMap<BlsPublicKey, ValidatorRegistration>>>,
}

impl IdentityBuilder {
    pub fn new(context: Context) -> Self {
        let signing_key = SecretKey::try_from([1u8; 32].as_ref()).unwrap();
        let public_key = signing_key.public_key();
        Self {
            signing_key,
            public_key,
            context: Arc::new(context),
            bids: Default::default(),
            registrations: Default::default(),
        }
    }
}

#[async_trait]
impl BlindedBlockProvider for IdentityBuilder {
    async fn register_validators(
        &self,
        registrations: &[SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let mut state = self.registrations.lock().unwrap();
        for registration in registrations {
            let registration = &registration.message;
            let public_key = registration.public_key.clone();
            state.insert(public_key, registration.clone());
        }
        Ok(())
    }

    async fn fetch_best_bid(
        &self,
        AuctionRequest { slot, parent_hash, public_key }: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error> {
        let capella_fork_slot = self.context.capella_fork_epoch * self.context.slots_per_epoch;
        let state = self.registrations.lock().unwrap();
        let preferences = state.get(public_key).unwrap();
        let value = U256::from(1337);
        let (payload, builder_bid) = if *slot < capella_fork_slot {
            let payload = bellatrix::ExecutionPayload {
                parent_hash: parent_hash.clone(),
                fee_recipient: preferences.fee_recipient.clone(),
                gas_limit: preferences.gas_limit,
                ..Default::default()
            };
            let header = ExecutionPayloadHeader::Bellatrix(
                bellatrix::ExecutionPayloadHeader::try_from(&payload).unwrap(),
            );
            let builder_bid = BuilderBid::Bellatrix(builder_bid::bellatrix::BuilderBid {
                header,
                value,
                public_key: self.public_key.clone(),
            });
            (ExecutionPayload::Bellatrix(payload), builder_bid)
        } else {
            let payload = capella::ExecutionPayload {
                parent_hash: parent_hash.clone(),
                fee_recipient: preferences.fee_recipient.clone(),
                gas_limit: preferences.gas_limit,
                ..Default::default()
            };
            let header = ExecutionPayloadHeader::Capella(
                capella::ExecutionPayloadHeader::try_from(&payload).unwrap(),
            );
            let builder_bid = BuilderBid::Capella(builder_bid::capella::BuilderBid {
                header,
                value,
                public_key: self.public_key.clone(),
            });
            (ExecutionPayload::Capella(payload), builder_bid)
        };

        let signature =
            sign_builder_message(&builder_bid, &self.signing_key, &self.context).unwrap();
        let signed_builder_bid = SignedBuilderBid { message: builder_bid, signature };
        let mut state = self.bids.lock().unwrap();
        state.insert(*slot, payload);
        Ok(signed_builder_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<AuctionContents, Error> {
        let slot = signed_block.message().slot();
        let state = self.bids.lock().unwrap();
        let execution_payload = state.get(&slot).cloned().unwrap();
        let auction_contents = match signed_block.message().version() {
            Fork::Bellatrix => AuctionContents::Bellatrix(execution_payload),
            Fork::Capella => AuctionContents::Capella(execution_payload),
            Fork::Deneb => unimplemented!(),
            _ => unreachable!("fork not reachable from this type"),
        };
        Ok(auction_contents)
    }
}

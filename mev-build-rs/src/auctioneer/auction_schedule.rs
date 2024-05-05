use ethereum_consensus::primitives::{BlsPublicKey, Slot};
use mev_rs::types::ProposerSchedule;
use reth::primitives::Address;
use std::collections::{HashMap, HashSet};

pub type RelayIndex = usize;
pub type RelaySet = HashSet<RelayIndex>;
pub type Proposals = HashMap<Proposer, RelaySet>;

#[derive(Debug, Clone, Default, Hash, PartialEq, Eq)]
pub struct Proposer {
    pub public_key: BlsPublicKey,
    pub fee_recipient: Address,
    pub gas_limit: u64,
}

#[derive(Debug, Default)]
pub struct AuctionSchedule {
    // TODO: unpack into flatter data structure(s)
    schedule: HashMap<Slot, Proposals>,
}

// TODO: merge w/ `ProposerSchedule`?
impl AuctionSchedule {
    pub fn clear(&mut self, retain_slot: Slot) {
        self.schedule.retain(|&slot, _| slot >= retain_slot);
    }

    pub fn get_matching_proposals(&self, slot: Slot) -> Option<&Proposals> {
        self.schedule.get(&slot)
    }

    pub fn process(&mut self, relay: RelayIndex, schedule: &[ProposerSchedule]) -> Vec<Slot> {
        let mut slots = Vec::with_capacity(schedule.len());
        for entry in schedule {
            slots.push(entry.slot);
            let slot = self.schedule.entry(entry.slot).or_default();
            let registration = &entry.entry.message;
            let proposer = Proposer {
                public_key: registration.public_key.clone(),
                fee_recipient: Address::from_slice(registration.fee_recipient.as_ref()),
                gas_limit: registration.gas_limit,
            };
            let relays = slot.entry(proposer).or_default();
            relays.insert(relay);
        }
        slots
    }
}

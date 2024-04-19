use ethereum_consensus::primitives::{BlsPublicKey, ExecutionAddress, Slot};
use mev_rs::{types::ProposerSchedule, Relay};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub type RelaySet = HashSet<Arc<Relay>>;
pub type Proposals = HashMap<Proposer, RelaySet>;

#[derive(Debug, Default, Hash, PartialEq, Eq)]
pub struct Proposer {
    pub public_key: BlsPublicKey,
    pub fee_recipient: ExecutionAddress,
    pub gas_limit: u64,
}

#[derive(Debug, Default)]
pub struct AuctionSchedule {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    // TODO: use CL to also restrict builds by proposer
    schedule: HashMap<Slot, Proposals>,
}

impl AuctionSchedule {
    pub fn clear(&self, retain_slot: Slot) {
        let mut state = self.state.lock();
        state.schedule.retain(|&slot, _| slot >= retain_slot);
    }

    pub fn take_matching_proposals(&self, slot: Slot) -> Option<Proposals> {
        let mut state = self.state.lock();
        state.schedule.remove(&slot)
    }

    pub fn process(&self, relay: Arc<Relay>, schedule: &[ProposerSchedule]) -> Vec<Slot> {
        let mut slots = Vec::with_capacity(schedule.len());
        let mut state = self.state.lock();
        for entry in schedule {
            slots.push(entry.slot);
            let slot = state.schedule.entry(entry.slot).or_default();
            let registration = &entry.entry.message;
            let proposer = Proposer {
                public_key: registration.public_key.clone(),
                fee_recipient: registration.fee_recipient.clone(),
                gas_limit: registration.gas_limit,
            };
            let relays = slot.entry(proposer).or_default();
            relays.insert(relay.clone());
        }
        slots
    }
}

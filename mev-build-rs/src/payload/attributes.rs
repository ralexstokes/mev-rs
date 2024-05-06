use crate::bidder::RevenueUpdate;
use reth::{
    api::PayloadBuilderAttributes,
    payload::{EthPayloadBuilderAttributes, PayloadId},
    primitives::{
        alloy_primitives::private::alloy_rlp::Encodable,
        revm_primitives::{BlockEnv, CfgEnvWithHandlerCfg},
        Address, ChainSpec, Header, Withdrawals, B256,
    },
    rpc::types::engine::PayloadAttributes,
};
use sha2::Digest;
use std::convert::Infallible;
use tokio::sync::mpsc::Sender;

pub fn payload_id_with_bytes(
    parent: &B256,
    attributes: &PayloadAttributes,
    proposal: Option<&ProposalAttributes>,
) -> (PayloadId, [u8; 8]) {
    let mut hasher = sha2::Sha256::new();
    hasher.update(parent.as_slice());
    hasher.update(&attributes.timestamp.to_be_bytes()[..]);
    hasher.update(attributes.prev_randao.as_slice());
    hasher.update(attributes.suggested_fee_recipient.as_slice());
    if let Some(withdrawals) = &attributes.withdrawals {
        let mut buf = Vec::new();
        withdrawals.encode(&mut buf);
        hasher.update(buf);
    }

    if let Some(parent_beacon_block) = attributes.parent_beacon_block_root {
        hasher.update(parent_beacon_block);
    }

    if let Some(proposal) = proposal {
        hasher.update(proposal.proposer_gas_limit.to_be_bytes());
        hasher.update(proposal.proposer_fee_recipient.as_slice());
    }

    let out = hasher.finalize();
    let inner: [u8; 8] = out.as_slice()[..8].try_into().expect("sufficient length");
    (PayloadId::new(inner), inner)
}

pub fn mix_proposal_into_payload_id(
    payload_id: [u8; 8],
    proposal: &ProposalAttributes,
) -> PayloadId {
    let mut hasher = sha2::Sha256::new();
    hasher.update(payload_id);

    hasher.update(proposal.proposer_gas_limit.to_be_bytes());
    hasher.update(proposal.proposer_fee_recipient.as_slice());

    let out = hasher.finalize();
    PayloadId::new(out.as_slice()[..8].try_into().expect("sufficient length"))
}

#[derive(Debug, Clone)]
pub struct ProposalAttributes {
    pub proposer_gas_limit: u64,
    pub proposer_fee_recipient: Address,
    pub bidder: Sender<RevenueUpdate>,
}

#[derive(Debug, Clone)]
pub struct BuilderPayloadBuilderAttributes {
    pub inner: EthPayloadBuilderAttributes,
    // TODO: can skip this if we expose the inner value upstream
    // NOTE: save this here to avoid recomputing later
    payload_id: Option<[u8; 8]>,
    pub proposal: Option<ProposalAttributes>,
}

impl BuilderPayloadBuilderAttributes {
    pub fn new(parent: B256, attributes: PayloadAttributes) -> Self {
        let (id, id_bytes) = payload_id_with_bytes(&parent, &attributes, None);

        let inner = EthPayloadBuilderAttributes {
            id,
            parent,
            timestamp: attributes.timestamp,
            suggested_fee_recipient: attributes.suggested_fee_recipient,
            prev_randao: attributes.prev_randao,
            withdrawals: attributes.withdrawals.unwrap_or_default().into(),
            parent_beacon_block_root: attributes.parent_beacon_block_root,
        };
        Self { inner, payload_id: Some(id_bytes), proposal: None }
    }

    pub fn attach_proposal(&mut self, proposal: ProposalAttributes) {
        // NOTE: error to call this more than once; see note on this field, hopefully this goes away
        if let Some(payload_id) = self.payload_id.take() {
            let id = mix_proposal_into_payload_id(payload_id, &proposal);
            self.inner.id = id;
            self.proposal = Some(proposal);
        }
    }
}

unsafe impl Send for BuilderPayloadBuilderAttributes {}
unsafe impl Sync for BuilderPayloadBuilderAttributes {}

impl PayloadBuilderAttributes for BuilderPayloadBuilderAttributes {
    type RpcPayloadAttributes = PayloadAttributes;
    type Error = Infallible;

    fn try_new(
        parent: B256,
        rpc_payload_attributes: Self::RpcPayloadAttributes,
    ) -> Result<Self, Self::Error> {
        Ok(Self::new(parent, rpc_payload_attributes))
    }

    fn payload_id(&self) -> PayloadId {
        self.inner.payload_id()
    }

    fn parent(&self) -> B256 {
        self.inner.parent
    }

    fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }

    fn parent_beacon_block_root(&self) -> Option<B256> {
        self.inner.parent_beacon_block_root
    }

    fn suggested_fee_recipient(&self) -> Address {
        self.inner.suggested_fee_recipient
    }

    fn prev_randao(&self) -> B256 {
        self.inner.prev_randao
    }

    fn withdrawals(&self) -> &Withdrawals {
        &self.inner.withdrawals
    }

    fn cfg_and_block_env(
        &self,
        chain_spec: &ChainSpec,
        parent: &Header,
    ) -> (CfgEnvWithHandlerCfg, BlockEnv) {
        self.inner.cfg_and_block_env(chain_spec, parent)
    }
}

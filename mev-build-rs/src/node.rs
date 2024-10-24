//! Customized types for the builder to configuring reth

use crate::payload::{
    attributes::BuilderPayloadBuilderAttributes, service_builder::PayloadServiceBuilder,
};
use reth::{
    api::{EngineTypes, FullNodeTypes, PayloadTypes},
    builder::{components::ComponentsBuilder, NodeTypes, NodeTypesWithEngine},
    chainspec::ChainSpec,
    payload::EthBuiltPayload,
    rpc::types::engine::{
        ExecutionPayloadEnvelopeV2, ExecutionPayloadEnvelopeV3, ExecutionPayloadEnvelopeV4,
        ExecutionPayloadV1, PayloadAttributes as EthPayloadAttributes,
    },
};
use reth_node_ethereum::node::{
    EthereumConsensusBuilder, EthereumExecutorBuilder, EthereumNetworkBuilder, EthereumPoolBuilder,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct BuilderNode;

impl BuilderNode {
    /// Returns a [ComponentsBuilder] configured for a regular Ethereum node.
    pub fn components_with<Node>(
        payload_service_builder: PayloadServiceBuilder,
    ) -> ComponentsBuilder<
        Node,
        EthereumPoolBuilder,
        PayloadServiceBuilder,
        EthereumNetworkBuilder,
        EthereumExecutorBuilder,
        EthereumConsensusBuilder,
    >
    where
        Node: FullNodeTypes<
            Types: NodeTypesWithEngine<Engine = BuilderEngineTypes, ChainSpec = ChainSpec>,
        >,
    {
        ComponentsBuilder::default()
            .node_types::<Node>()
            .pool(EthereumPoolBuilder::default())
            .payload(payload_service_builder)
            .network(EthereumNetworkBuilder::default())
            .executor(EthereumExecutorBuilder::default())
            .consensus(EthereumConsensusBuilder::default())
    }
}

impl NodeTypes for BuilderNode {
    type Primitives = ();
    type ChainSpec = ChainSpec;
}

impl NodeTypesWithEngine for BuilderNode {
    type Engine = BuilderEngineTypes;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BuilderEngineTypes;

impl PayloadTypes for BuilderEngineTypes {
    type BuiltPayload = EthBuiltPayload;
    type PayloadAttributes = EthPayloadAttributes;
    type PayloadBuilderAttributes = BuilderPayloadBuilderAttributes;
}

impl EngineTypes for BuilderEngineTypes {
    type ExecutionPayloadV1 = ExecutionPayloadV1;
    type ExecutionPayloadEnvelopeV2 = ExecutionPayloadEnvelopeV2;
    type ExecutionPayloadEnvelopeV3 = ExecutionPayloadEnvelopeV3;
    type ExecutionPayloadEnvelopeV4 = ExecutionPayloadEnvelopeV4;
}

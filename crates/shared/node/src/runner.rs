//! Contains the [`BaseNodeRunner`], which is responsible for configuring and launching a Base node.

use std::fmt;

use eyre::Result;
use reth_node_builder::{
    EngineNodeLauncher, Node, NodeHandleFor, TreeConfig, components::BasicPayloadServiceBuilder,
};
use reth_optimism_node::{args::RollupArgs, node::OpPayloadBuilder};
use reth_optimism_payload_builder::config::{OpDAConfig, OpGasLimitConfig};
use reth_provider::providers::BlockchainProvider;
use tracing::info;

use crate::{
    BaseBuilder, BaseNodeBuilder, BaseNodeExtension, BaseNodeHandle, FromExtensionConfig,
    node::BaseNode,
};

/// Configuration for launching a Base node with custom components.
#[derive(Debug, Clone)]
pub struct BaseNodeConfig<P> {
    /// Rollup-specific arguments.
    pub rollup_args: RollupArgs,
    /// DA configuration.
    pub da_config: OpDAConfig,
    /// Gas limit configuration.
    pub gas_limit_config: OpGasLimitConfig,
    /// Payload builder to use.
    pub payload_builder: P,
}

impl<P> BaseNodeConfig<P> {
    /// Creates a new config with the specified rollup args and payload builder.
    pub fn new(rollup_args: RollupArgs, payload_builder: P) -> Self {
        Self {
            rollup_args,
            da_config: OpDAConfig::default(),
            gas_limit_config: OpGasLimitConfig::default(),
            payload_builder,
        }
    }

    /// Configure the data availability configuration.
    pub fn with_da_config(mut self, da_config: OpDAConfig) -> Self {
        self.da_config = da_config;
        self
    }

    /// Configure the gas limit configuration.
    pub fn with_gas_limit_config(mut self, gas_limit_config: OpGasLimitConfig) -> Self {
        self.gas_limit_config = gas_limit_config;
        self
    }
}

/// Wraps the Base node configuration and orchestrates builder wiring.
///
/// The runner is generic over the payload builder type `P`, allowing both the node
/// (which uses `BasicPayloadServiceBuilder<OpPayloadBuilder>`) and the builder
/// (which uses `FlashblocksServiceBuilder`) to share the same scaffolding.
pub struct BaseNodeRunner<P> {
    /// Node configuration including the payload builder.
    config: BaseNodeConfig<P>,
    /// Registered builder extensions.
    extensions: Vec<Box<dyn BaseNodeExtension>>,
}

impl<P: fmt::Debug> fmt::Debug for BaseNodeRunner<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BaseNodeRunner")
            .field("config", &self.config)
            .field("extensions", &self.extensions)
            .finish()
    }
}

impl BaseNodeRunner<BasicPayloadServiceBuilder<OpPayloadBuilder>> {
    /// Creates a runner with the default `OpPayloadBuilder`.
    ///
    /// This is the convenience constructor for the node binary.
    pub fn with_default_payload(rollup_args: RollupArgs) -> Self {
        let payload_builder = BasicPayloadServiceBuilder::new(
            OpPayloadBuilder::new(rollup_args.compute_pending_block)
                .with_da_config(OpDAConfig::default())
                .with_gas_limit_config(OpGasLimitConfig::default()),
        );
        Self::new(BaseNodeConfig::new(rollup_args, payload_builder))
    }

    /// Creates a new launcher using the provided rollup arguments.
    ///
    /// This is a backward-compatible constructor that matches the old API.
    pub fn from_rollup_args(rollup_args: RollupArgs) -> Self {
        Self::with_default_payload(rollup_args)
    }
}

impl<P> BaseNodeRunner<P> {
    /// Creates a new runner with the provided configuration.
    pub fn new(config: BaseNodeConfig<P>) -> Self {
        Self { config, extensions: Vec::new() }
    }

    /// Registers a new builder extension.
    pub fn install_ext<T: FromExtensionConfig + 'static>(&mut self, config: T::Config) {
        self.extensions.push(Box::new(T::from_config(config)));
    }

    /// Returns a reference to the rollup args.
    pub const fn rollup_args(&self) -> &RollupArgs {
        &self.config.rollup_args
    }
}

impl BaseNodeRunner<BasicPayloadServiceBuilder<OpPayloadBuilder>> {
    /// Applies all Base-specific wiring to the supplied builder, launches the node, and returns a
    /// handle that can be awaited.
    ///
    /// This implementation uses the default `OpPayloadBuilder`.
    pub fn run(self, builder: BaseNodeBuilder) -> BaseNodeHandle {
        let Self { config, extensions } = self;
        BaseNodeHandle::new(Self::launch_node_default(config, extensions, builder))
    }

    async fn launch_node_default(
        config: BaseNodeConfig<BasicPayloadServiceBuilder<OpPayloadBuilder>>,
        extensions: Vec<Box<dyn BaseNodeExtension>>,
        builder: BaseNodeBuilder,
    ) -> Result<NodeHandleFor<BaseNode>> {
        info!(target: "base-runner", "starting custom Base node");

        let BaseNodeConfig { rollup_args, da_config, gas_limit_config, payload_builder: _ } =
            config;

        let base_node = BaseNode::new(rollup_args)
            .with_da_config(da_config)
            .with_gas_limit_config(gas_limit_config);

        let builder = builder
            .with_types_and_provider::<BaseNode, BlockchainProvider<_>>()
            .with_components(base_node.components())
            .with_add_ons(base_node.add_ons())
            .on_component_initialized(move |_ctx| Ok(()));

        let builder = extensions
            .into_iter()
            .fold(BaseBuilder::new(builder), |builder, extension| extension.apply(builder))
            .add_node_started_hook(|_| {
                base_cli_utils::register_version_metrics!();
                Ok(())
            });

        builder
            .launch_with_fn(|builder| {
                let engine_tree_config = TreeConfig::default()
                    .with_persistence_threshold(builder.config().engine.persistence_threshold)
                    .with_memory_block_buffer_target(
                        builder.config().engine.memory_block_buffer_target,
                    );

                let launcher = EngineNodeLauncher::new(
                    builder.task_executor().clone(),
                    builder.config().datadir(),
                    engine_tree_config,
                );

                builder.launch_with(launcher)
            })
            .await
    }
}

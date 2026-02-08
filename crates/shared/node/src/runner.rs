//! Contains the [`NodeRunner`], which is responsible for configuring and launching a Base node.

use eyre::Result;
use reth_node_builder::{EngineNodeLauncher, Node, NodeHandle, NodeHandleFor, TreeConfig};
use reth_provider::providers::BlockchainProvider;
use tracing::info;

use crate::{
    BaseNodeBuilder, BaseNodeExtension, FromExtensionConfig, NodeHooks,
    node::BaseNode,
    service::{DefaultPayloadServiceBuilder, PayloadServiceBuilder},
};

/// Wraps the Base node configuration and orchestrates builder wiring.
#[derive(Debug)]
pub struct NodeRunner<SB: PayloadServiceBuilder = DefaultPayloadServiceBuilder> {
    /// Rollup-specific arguments forwarded to the Optimism node implementation.
    rollup_args: reth_optimism_node::args::RollupArgs,
    /// Registered builder extensions.
    extensions: Vec<Box<dyn BaseNodeExtension>>,
    /// Payload service builder.
    service_builder: SB,
}

impl NodeRunner<DefaultPayloadServiceBuilder> {
    /// Creates a new launcher using the provided rollup arguments.
    pub fn new(rollup_args: reth_optimism_node::args::RollupArgs) -> Self {
        Self { rollup_args, extensions: Vec::new(), service_builder: DefaultPayloadServiceBuilder }
    }
}

impl<SB: PayloadServiceBuilder> NodeRunner<SB> {
    /// Swap the payload service builder.
    pub fn with_service_builder<SB2: PayloadServiceBuilder>(self, sb: SB2) -> NodeRunner<SB2> {
        NodeRunner {
            rollup_args: self.rollup_args,
            extensions: self.extensions,
            service_builder: sb,
        }
    }

    /// Registers a new builder extension.
    pub fn install_ext<T: FromExtensionConfig + 'static>(&mut self, config: T::Config) {
        self.extensions.push(Box::new(T::from_config(config)));
    }

    /// Applies all Base-specific wiring to the supplied builder, launches the node, and waits for
    /// shutdown.
    pub async fn run(self, builder: BaseNodeBuilder) -> Result<()> {
        let Self { rollup_args, extensions, service_builder } = self;
        let NodeHandle { node: _node, node_exit_future } =
            Self::launch_node(rollup_args, extensions, service_builder, builder).await?;
        node_exit_future.await?;
        Ok(())
    }

    async fn launch_node(
        rollup_args: reth_optimism_node::args::RollupArgs,
        extensions: Vec<Box<dyn BaseNodeExtension>>,
        service_builder: SB,
        builder: BaseNodeBuilder,
    ) -> Result<NodeHandleFor<BaseNode>> {
        info!(target: "base-runner", "starting custom Base node");

        let base_node = BaseNode::new(rollup_args);
        let components = service_builder.build_components(&base_node);

        let configured = builder
            .with_types_and_provider::<BaseNode, BlockchainProvider<_>>()
            .with_components(components)
            .with_add_ons(base_node.add_ons())
            .on_component_initialized(move |_ctx| Ok(()));

        // Collect hooks from extensions
        let hooks = extensions
            .into_iter()
            .fold(NodeHooks::new(), |b, ext| ext.apply(b))
            .add_node_started_hook(|_| {
                base_cli_utils::register_version_metrics!();
                Ok(())
            });

        // Apply hooks to configured builder
        let configured = hooks.apply_to(configured);

        // Launch with EngineNodeLauncher
        let engine_tree_config = TreeConfig::default()
            .with_persistence_threshold(configured.config().engine.persistence_threshold)
            .with_memory_block_buffer_target(configured.config().engine.memory_block_buffer_target);

        let launcher = EngineNodeLauncher::new(
            configured.task_executor().clone(),
            configured.config().datadir(),
            engine_tree_config,
        );

        configured.launch_with(launcher).await
    }
}

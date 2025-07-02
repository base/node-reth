use base_reth_flashblocks_rpc::{cache::Cache, flashblocks::FlashblocksClient, rpc::EthApiExt};
use std::sync::Arc;
use std::time::Duration;

use base_reth_flashblocks_rpc::rpc::EthApiOverrideServer;
use clap::Parser;
use mempool_event_publisher::{KafkaConfig, MempoolEventPublisher, PoolWrapper};
use reth::builder::{Node, NodeComponents};
use reth::{
    builder::{EngineNodeLauncher, TreeConfig},
    providers::providers::BlockchainProvider,
    transaction_pool::TransactionPool,
};
use reth_optimism_cli::{chainspec::OpChainSpecParser, Cli};
use reth_optimism_node::args::RollupArgs;
use reth_optimism_node::OpNode;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
#[command(next_help_heading = "Rollup")]
struct FlashblocksRollupArgs {
    #[command(flatten)]
    pub rollup_args: RollupArgs,

    #[arg(long = "websocket-url", value_name = "WEBSOCKET_URL")]
    pub websocket_url: Option<String>,

    #[arg(
        long = "receipt-buffer-size",
        value_name = "RECEIPT_BUFFER_SIZE",
        default_value = "2000",
        env = "RECEIPT_BUFFER_SIZE"
    )]
    pub receipt_buffer_size: usize,

    #[arg(
        long = "total-timeout-secs",
        value_name = "TOTAL_TIMEOUT_SECS",
        default_value = "4",
        env = "TOTAL_TIMEOUT_SECS"
    )]
    pub total_timeout_secs: u64,

    // Mempool event publisher configuration
    #[arg(
        long = "kafka-brokers",
        value_name = "KAFKA_BROKERS",
        env = "KAFKA_BROKERS"
    )]
    pub kafka_brokers: Option<String>,

    #[arg(
        long = "kafka-topic",
        value_name = "KAFKA_TOPIC",
        default_value = "mempool-events",
        env = "KAFKA_TOPIC"
    )]
    pub kafka_topic: String,

    #[arg(
        long = "kafka-client-id",
        value_name = "KAFKA_CLIENT_ID",
        default_value = "base-reth-mempool-publisher",
        env = "KAFKA_CLIENT_ID"
    )]
    pub kafka_client_id: String,
}

impl FlashblocksRollupArgs {
    fn flashblocks_enabled(&self) -> bool {
        self.websocket_url.is_some()
    }

    fn mempool_publisher_enabled(&self) -> bool {
        self.kafka_brokers.is_some()
    }

    fn create_kafka_config(&self) -> Option<KafkaConfig> {
        self.kafka_brokers.as_ref().map(|brokers| {
            KafkaConfig::new(brokers, &self.kafka_topic).with_client_id(&self.kafka_client_id)
        })
    }
}

fn main() {
    Cli::<OpChainSpecParser, FlashblocksRollupArgs>::parse()
        .run(|builder, flashblocks_rollup_args| async move {
            info!(message = "starting custom Base node");

            let cache = Arc::new(Cache::default());
            let cache_clone = Arc::new(Cache::default());
            let op_node = OpNode::new(flashblocks_rollup_args.rollup_args.clone());
            let receipt_buffer_size = flashblocks_rollup_args.receipt_buffer_size;
            let total_timeout_secs = flashblocks_rollup_args.total_timeout_secs;
            let chain_spec = builder.config().chain.clone();
            let flashblocks_enabled = flashblocks_rollup_args.flashblocks_enabled();
            let mempool_publisher_enabled = flashblocks_rollup_args.mempool_publisher_enabled();
            let kafka_config = flashblocks_rollup_args.create_kafka_config();

            let handle = builder
                .with_types_and_provider::<OpNode, BlockchainProvider<_>>()
                .with_components(op_node.components())
                .with_add_ons(op_node.add_ons())
                .on_component_initialized(move |ctx| {
                    if mempool_publisher_enabled {
                        if let Some(config) = kafka_config.clone() {
                            info!("Starting mempool event publisher");

                            let pool = ctx.components.pool().clone();
                            let all_events = pool.all_transactions_event_listener();
                            let lookup = Arc::new(PoolWrapper::new(pool.clone()));

                            let task_executor = ctx.task_executor.clone();

                            task_executor.spawn(async move {
                                match MempoolEventPublisher::new(config, lookup) {
                                    Ok(publisher) => {
                                        if let Err(e) = publisher.start(all_events).await {
                                            error!("Mempool event publisher failed: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to create mempool event publisher: {}", e);
                                    }
                                }
                            });
                        }
                    } else {
                        info!("Mempool event publisher is disabled");
                    }
                    Ok(())
                })
                .extend_rpc_modules(move |ctx| {
                    if flashblocks_enabled {
                        info!(message = "starting flashblocks integration");
                        let mut flashblocks_client =
                            FlashblocksClient::new(cache.clone(), receipt_buffer_size);

                        flashblocks_client
                            .init(flashblocks_rollup_args.websocket_url.unwrap().clone())
                            .unwrap();

                        let api_ext = EthApiExt::new(
                            ctx.registry.eth_api().clone(),
                            cache.clone(),
                            chain_spec.clone(),
                            flashblocks_client,
                            total_timeout_secs,
                        );
                        ctx.modules.replace_configured(api_ext.into_rpc())?;
                    } else {
                        info!(message = "flashblocks integration is disabled");
                    }
                    Ok(())
                })
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

                    if flashblocks_enabled {
                        builder.task_executor().spawn(async move {
                            let mut interval = tokio::time::interval(Duration::from_secs(2));
                            loop {
                                interval.tick().await;
                                cache_clone.cleanup_expired();
                            }
                        });
                    }
                    builder.launch_with(launcher)
                })
                .await?;

            handle.wait_for_node_exit().await
        })
        .unwrap();
}

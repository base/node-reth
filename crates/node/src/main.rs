use base_reth_flashblocks_rpc::rpc::EthApiExt;
use futures_util::TryStreamExt;
use reth::version::{
    default_reth_version_metadata, try_init_version_metadata, RethCliVersionConsts,
};
use reth_exex::ExExEvent;
use std::cell::{LazyCell, OnceCell};
use std::sync::Arc;

use base_reth_flashblocks_rpc::rpc::EthApiOverrideServer;
use base_reth_flashblocks_rpc::state::FlashblocksState;
use base_reth_flashblocks_rpc::subscription::FlashblocksSubscriber;
use base_reth_metering::{MeteringApiImpl, MeteringApiServer};
use base_reth_transaction_tracing::transaction_tracing_exex;
use clap::Parser;
use reth::builder::{Node, NodeHandle};
use reth::{
    builder::{EngineNodeLauncher, TreeConfig},
    providers::providers::BlockchainProvider,
};
use reth_optimism_cli::{chainspec::OpChainSpecParser, Cli};
use reth_optimism_exex::OpProofsExEx;
use reth_optimism_node::args::RollupArgs;
use reth_optimism_node::OpNode;
use tracing::info;
use url::Url;

pub const NODE_RETH_CLIENT_VERSION: &str = concat!("base/v", env!("CARGO_PKG_VERSION"));

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
#[command(next_help_heading = "Rollup")]
struct Args {
    #[command(flatten)]
    pub rollup_args: RollupArgs,

    #[arg(long = "websocket-url", value_name = "WEBSOCKET_URL")]
    pub websocket_url: Option<String>,

    /// Enable transaction tracing ExEx for mempool-to-block timing analysis
    #[arg(
        long = "enable-transaction-tracing",
        value_name = "ENABLE_TRANSACTION_TRACING"
    )]
    pub enable_transaction_tracing: bool,

    /// Enable `info` logs for transaction tracing
    #[arg(
        long = "enable-transaction-tracing-logs",
        value_name = "ENABLE_TRANSACTION_TRACING_LOGS"
    )]
    pub enable_transaction_tracing_logs: bool,

    /// Enable metering RPC for transaction bundle simulation
    #[arg(long = "enable-metering", value_name = "ENABLE_METERING")]
    pub enable_metering: bool,

    /// If true, initialize external-proofs exex to save and serve trie nodes to provide proofs
    /// faster.
    #[arg(
        long = "proofs-history",
        value_name = "PROOFS_HISTORY",
        default_value = "false"
    )]
    pub proofs_history: bool,

    /// The path to the storage DB for proofs history.
    #[arg(
        long = "proofs-history.storage-path",
        value_name = "PROOFS_HISTORY_STORAGE_PATH",
        required_if_eq("proofs-history", "true")
    )]
    pub proofs_history_storage_path: Option<PathBuf>,
}

impl Args {
    fn flashblocks_enabled(&self) -> bool {
        self.websocket_url.is_some()
    }
}

fn main() {
    let default_version_metadata = default_reth_version_metadata();
    try_init_version_metadata(RethCliVersionConsts {
        name_client: "Base Reth Node".to_string().into(),
        cargo_pkg_version: format!(
            "{}/{}",
            default_version_metadata.cargo_pkg_version,
            env!("CARGO_PKG_VERSION")
        )
        .into(),
        p2p_client_version: format!(
            "{}/{}",
            default_version_metadata.p2p_client_version, NODE_RETH_CLIENT_VERSION
        )
        .into(),
        extra_data: format!(
            "{}/{}",
            default_version_metadata.extra_data, NODE_RETH_CLIENT_VERSION
        )
        .into(),
        ..default_version_metadata
    })
    .expect("Unable to init version metadata");

    Cli::<OpChainSpecParser, Args>::parse()
        .run(|builder, args| async move {
            info!(message = "starting custom Base node");

            let flashblocks_enabled = args.flashblocks_enabled();
            let proofs_history_enabled = args.proofs_history;

            let transaction_tracing_enabled = args.enable_transaction_tracing;
            let metering_enabled = args.enable_metering;
            let op_node = OpNode::new(args.rollup_args.clone());

            let fb_cell: Arc<OnceCell<Arc<FlashblocksState<_>>>> = Arc::new(OnceCell::new());

            let proofs_storage_cell: Arc<LazyCell<Result<Arc<MdbxProofsStorage<_>>, eyre::Error>>> =
                Arc::new(LazyCell::new(|| {
                    let path = args
                        .proofs_history_storage_path
                        .expect("path must be set when proofs history is enabled");
                    Arc::new(
                        MdbxProofsStorage::new(args.proofs_history_storage_path.unwrap())
                            .map_err(|e| eyre::eyre!("Failed to create MdbxProofsStorage: {e}"))?,
                    )
                    .into()
                }));

            let NodeHandle {
                node: _node,
                node_exit_future,
            } = builder
                .with_types_and_provider::<OpNode, BlockchainProvider<_>>()
                .with_components(op_node.components())
                .with_add_ons(op_node.add_ons())
                .on_component_initialized(move |_ctx| Ok(()))
                .install_exex_if(
                    transaction_tracing_enabled,
                    "transaction-tracing",
                    move |ctx| async move {
                        Ok(transaction_tracing_exex(
                            ctx,
                            args.enable_transaction_tracing_logs,
                        ))
                    },
                )
                .install_exex_if(flashblocks_enabled, "flashblocks-canon", {
                    let fb_cell = fb_cell.clone();
                    move |mut ctx| async move {
                        let fb = fb_cell
                            .get_or_init(|| Arc::new(FlashblocksState::new(ctx.provider().clone())))
                            .clone();
                        Ok(async move {
                            while let Some(note) = ctx.notifications.try_next().await? {
                                if let Some(committed) = note.committed_chain() {
                                    for b in committed.blocks_iter() {
                                        fb.on_canonical_block_received(b);
                                    }
                                    let _ = ctx.events.send(ExExEvent::FinishedHeight(
                                        committed.tip().num_hash(),
                                    ));
                                }
                            }
                            Ok(())
                        })
                    }
                })
                .install_exex_if(
                    proofs_history_enabled,
                    "proofs-history",
                    async move |exex_context| {
                        Ok(OpProofsExEx::new(
                            exex_context,
                            storage_exec,
                            args.proofs_history_window,
                        )
                        .run()
                        .boxed())
                    },
                )
                .extend_rpc_modules(move |ctx| {
                    if proofs_history_enabled {
                        let api_ext =
                            EthApiExt::new(ctx.registry.eth_api().clone(), storage_rpc.clone());
                        let debug_ext = DebugApiExt::new(
                            ctx.node().provider().clone(),
                            ctx.registry.eth_api().clone(),
                            storage_rpc,
                            Box::new(ctx.node().task_executor().clone()),
                            ctx.node().evm_config().clone(),
                        );
                        ctx.modules.replace_configured(api_ext.into_rpc())?;
                        ctx.modules.replace_configured(debug_ext.into_rpc())?;
                    }
                    Ok(())
                })
                .extend_rpc_modules(move |ctx| {
                    if metering_enabled {
                        info!(message = "Starting Metering RPC");
                        let metering_api = MeteringApiImpl::new(ctx.provider().clone());
                        ctx.modules.merge_configured(metering_api.into_rpc())?;
                    }

                    if flashblocks_enabled {
                        info!(message = "Starting Flashblocks");

                        let ws_url = Url::parse(
                            args.websocket_url
                                .expect("WEBSOCKET_URL must be set when Flashblocks is enabled")
                                .as_str(),
                        )?;

                        let fb = fb_cell
                            .get_or_init(|| Arc::new(FlashblocksState::new(ctx.provider().clone())))
                            .clone();
                        fb.start();

                        let mut flashblocks_client = FlashblocksSubscriber::new(fb.clone(), ws_url);
                        flashblocks_client.start();

                        let api_ext = EthApiExt::new(
                            ctx.registry.eth_api().clone(),
                            ctx.registry.eth_handlers().filter.clone(),
                            fb,
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

                    builder.launch_with(launcher)
                })
                .await?;

            node_exit_future.await
        })
        .unwrap();
}

#![doc = include_str!("../README.md")]
#![doc(issue_tracker_base_url = "https://github.com/base/base/issues/")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

pub mod cli;
mod extension;

use base_builder_core::{BuilderConfig, FlashblocksServiceBuilder};
use base_client_node::NodeRunner;
use extension::TxDataStoreExtension;
use reth_optimism_cli::{Cli, chainspec::OpChainSpecParser};

type BuilderCli = Cli<OpChainSpecParser, cli::Args>;

#[global_allocator]
static ALLOC: reth_cli_util::allocator::Allocator = reth_cli_util::allocator::new_allocator();

fn main() {
    base_cli_utils::init_common!();
    base_cli_utils::init_reth!();

    let cli = base_cli_utils::parse_cli!(BuilderCli);

    cli.run(|builder, builder_args| async move {
        let builder_config = BuilderConfig::try_from(builder_args.clone())?;

        let tx_data_store = builder_config.tx_data_store.clone();

        let mut runner = NodeRunner::new(builder_args.rollup_args.clone())
            .with_service_builder(FlashblocksServiceBuilder(builder_config));
        runner.install_ext::<TxDataStoreExtension>(tx_data_store);

        runner.run(builder).await
    })
    .unwrap();
}

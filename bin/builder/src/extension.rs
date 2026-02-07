//! Builder-specific node extensions.

use base_builder_core::{BaseApiExtServer, TxDataStore, TxDataStoreExt};
use base_client_node::{BaseNodeExtension, BaseRpcContext, FromExtensionConfig, NodeHooks};

/// Extension that registers the [`TxDataStoreExt`] RPC module.
#[derive(Debug)]
pub(crate) struct TxDataStoreExtension {
    tx_data_store: TxDataStore,
}

impl BaseNodeExtension for TxDataStoreExtension {
    fn apply(self: Box<Self>, builder: NodeHooks) -> NodeHooks {
        let tx_data_store = self.tx_data_store;
        builder.add_rpc_module(move |ctx: &mut BaseRpcContext<'_>| {
            let ext = TxDataStoreExt::new(tx_data_store);
            ctx.modules.add_or_replace_configured(ext.into_rpc())?;
            Ok(())
        })
    }
}

impl FromExtensionConfig for TxDataStoreExtension {
    type Config = TxDataStore;

    fn from_config(config: Self::Config) -> Self {
        Self { tx_data_store: config }
    }
}

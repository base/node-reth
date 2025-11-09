use alloy_consensus::{Header, Sealed};
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::U256;
use base_reth_flashblocks_rpc::rpc::{FlashblocksAPI, PendingBlocksAPI};
use jsonrpsee::{
    core::{async_trait, RpcResult},
    proc_macros::rpc,
};
use reth::providers::BlockReaderIdExt;
use reth_optimism_chainspec::OpChainSpec;
use reth_primitives_traits::SealedHeader;
use reth_provider::{ChainSpecProvider, StateProviderFactory};
use std::sync::Arc;
use tips_core::types::{Bundle, BundleWithMetadata, MeterBundleResponse};
use tracing::{error, info};

use crate::{meter_bundle, FlashblockTrieCache};

/// RPC API for transaction metering
#[rpc(server, namespace = "base")]
pub trait MeteringApi {
    /// Simulates and meters a bundle of transactions
    #[method(name = "meterBundle")]
    async fn meter_bundle(&self, bundle: Bundle) -> RpcResult<MeterBundleResponse>;
}

/// Implementation of the metering RPC API
pub struct MeteringApiImpl<Provider, FB> {
    provider: Provider,
    flashblocks_state: Arc<FB>,
    /// Single-entry cache for the latest flashblock's trie nodes
    trie_cache: FlashblockTrieCache,
}

impl<Provider, FB> MeteringApiImpl<Provider, FB>
where
    Provider: StateProviderFactory
        + ChainSpecProvider<ChainSpec = OpChainSpec>
        + BlockReaderIdExt<Header = Header>
        + Clone,
    FB: FlashblocksAPI,
{
    /// Creates a new instance of MeteringApi
    pub fn new(provider: Provider, flashblocks_state: Arc<FB>) -> Self {
        Self {
            provider,
            flashblocks_state,
            trie_cache: FlashblockTrieCache::new(),
        }
    }
}

#[async_trait]
impl<Provider, FB> MeteringApiServer for MeteringApiImpl<Provider, FB>
where
    Provider: StateProviderFactory
        + ChainSpecProvider<ChainSpec = OpChainSpec>
        + BlockReaderIdExt<Header = Header>
        + Clone
        + Send
        + Sync
        + 'static,
    FB: FlashblocksAPI + Send + Sync + 'static,
{
    async fn meter_bundle(&self, bundle: Bundle) -> RpcResult<MeterBundleResponse> {
        info!(
            num_transactions = bundle.txs.len(),
            block_number = bundle.block_number,
            "Starting bundle metering"
        );

        // Get pending flashblocks state
        let pending_blocks = self.flashblocks_state.get_pending_blocks();

        // Get header and flashblock index from pending blocks
        // If no pending blocks exist, fall back to latest canonical block
        let (header, flashblock_index, canonical_block_number) =
            if let Some(pb) = pending_blocks.as_ref() {
                let latest_header: Sealed<Header> = pb.latest_header();
                let flashblock_index = pb.latest_flashblock_index();
                let canonical_block_number = pb.canonical_block_number();

                info!(
                    latest_block = latest_header.number,
                    canonical_block = %canonical_block_number,
                    flashblock_index = flashblock_index,
                    "Using latest flashblock state for metering"
                );

                // Convert Sealed<Header> to SealedHeader
                let sealed_header =
                    SealedHeader::new(latest_header.inner().clone(), latest_header.hash());
                (sealed_header, flashblock_index, canonical_block_number)
            } else {
                // No pending blocks, use latest canonical block
                let canonical_block_number = pending_blocks.get_canonical_block_number();
                let header = self
                    .provider
                    .sealed_header_by_number_or_tag(canonical_block_number)
                    .map_err(|e| {
                        jsonrpsee::types::ErrorObjectOwned::owned(
                            jsonrpsee::types::ErrorCode::InternalError.code(),
                            format!("Failed to get canonical block header: {}", e),
                            None::<()>,
                        )
                    })?
                    .ok_or_else(|| {
                        jsonrpsee::types::ErrorObjectOwned::owned(
                            jsonrpsee::types::ErrorCode::InternalError.code(),
                            "Canonical block not found".to_string(),
                            None::<()>,
                        )
                    })?;

                info!(
                    canonical_block = header.number,
                    "No flashblocks available, using canonical block state for metering"
                );

                (header, 0, canonical_block_number)
            };

        // Manually decode transactions to OpTxEnvelope (op-alloy 0.20) instead of using
        // BundleWithMetadata.transactions() which returns op-alloy 0.21 types incompatible with reth.
        // TODO: Remove this workaround after reth updates to op-alloy 0.21 (already on main, awaiting release)
        let mut decoded_txs = Vec::new();
        for tx_bytes in &bundle.txs {
            let mut reader = tx_bytes.as_ref();
            let tx = op_alloy_consensus::OpTxEnvelope::decode_2718(&mut reader).map_err(|e| {
                jsonrpsee::types::ErrorObjectOwned::owned(
                    jsonrpsee::types::ErrorCode::InvalidParams.code(),
                    format!("Failed to decode transaction: {}", e),
                    None::<()>,
                )
            })?;
            decoded_txs.push(tx);
        }

        let bundle_with_metadata = BundleWithMetadata::load(bundle.clone()).map_err(|e| {
            jsonrpsee::types::ErrorObjectOwned::owned(
                jsonrpsee::types::ErrorCode::InvalidParams.code(),
                format!("Failed to load bundle metadata: {}", e),
                None::<()>,
            )
        })?;

        // Get state provider for the canonical block
        let state_provider = self
            .provider
            .state_by_block_number_or_tag(canonical_block_number)
            .map_err(|e| {
                error!(error = %e, "Failed to get state provider");
                jsonrpsee::types::ErrorObjectOwned::owned(
                    jsonrpsee::types::ErrorCode::InternalError.code(),
                    format!("Failed to get state provider: {}", e),
                    None::<()>,
                )
            })?;

        // If we have pending flashblocks, get the state to apply pending changes
        let flashblocks_state = pending_blocks.as_ref().map(|pb| crate::FlashblocksState {
            cache: pb.get_db_cache(),
            bundle_state: pb.get_bundle_state(),
        });

        // Get the flashblock index if we have pending flashblocks
        let state_flashblock_index = pending_blocks
            .as_ref()
            .map(|pb| pb.latest_flashblock_index());

        // If we have flashblocks, ensure the trie is cached and get it
        let cached_trie = if let Some(ref fb_state) = flashblocks_state {
            let fb_index = state_flashblock_index.unwrap();

            // Ensure the flashblock trie is cached and return it
            Some(
                self.trie_cache
                    .ensure_cached(header.hash(), fb_index, fb_state, &*state_provider)
                    .map_err(|e| {
                        error!(error = %e, "Failed to cache flashblock trie");
                        jsonrpsee::types::ErrorObjectOwned::owned(
                            jsonrpsee::types::ErrorCode::InternalError.code(),
                            format!("Failed to cache flashblock trie: {}", e),
                            None::<()>,
                        )
                    })?,
            )
        } else {
            None
        };

        // Meter bundle using utility function
        let result = meter_bundle(
            state_provider,
            self.provider.chain_spec().clone(),
            decoded_txs,
            &header,
            &bundle_with_metadata,
            flashblocks_state,
            cached_trie,
        )
        .map_err(|e| {
            error!(error = %e, "Bundle metering failed");
            jsonrpsee::types::ErrorObjectOwned::owned(
                jsonrpsee::types::ErrorCode::InternalError.code(),
                format!("Bundle metering failed: {}", e),
                None::<()>,
            )
        })?;

        // Calculate average gas price
        let bundle_gas_price = if result.total_gas_used > 0 {
            (result.total_gas_fees / U256::from(result.total_gas_used)).to_string()
        } else {
            "0".to_string()
        };

        info!(
            bundle_hash = %result.bundle_hash,
            num_transactions = result.results.len(),
            total_gas_used = result.total_gas_used,
            total_time_us = result.total_time_us,
            state_root_time_us = result.state_root_time_us,
            state_block_number = header.number,
            flashblock_index = flashblock_index,
            "Bundle metering completed successfully"
        );

        Ok(MeterBundleResponse {
            bundle_gas_price,
            bundle_hash: result.bundle_hash,
            coinbase_diff: result.total_gas_fees.to_string(),
            eth_sent_to_coinbase: "0".to_string(),
            gas_fees: result.total_gas_fees.to_string(),
            results: result.results,
            state_block_number: header.number,
            state_flashblock_index,
            total_gas_used: result.total_gas_used,
            // TODO: Rename to total_time_us in tips-core.
            total_execution_time_us: result.total_time_us,
            state_root_time_us: result.state_root_time_us,
        })
    }
}

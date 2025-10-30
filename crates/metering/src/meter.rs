use alloy_consensus::{transaction::SignerRecoverable, BlockHeader, Transaction as _};
use alloy_primitives::{B256, U256};
use eyre::{eyre, Result as EyreResult};
use reth::revm::db::{BundleState, Cache, CacheDB, State};
use reth_evm::execute::BlockBuilder;
use reth_evm::ConfigureEvm;
use reth_optimism_chainspec::OpChainSpec;
use reth_optimism_evm::{OpEvmConfig, OpNextBlockEnvAttributes};
use reth_primitives_traits::SealedHeader;
use reth_trie_common::TrieInput;
use revm_database::states::bundle_state::BundleRetention;
use std::sync::Arc;
use std::time::Instant;

use crate::TransactionResult;

/// State from pending flashblocks that is used as a base for metering
#[derive(Debug, Clone)]
pub struct FlashblocksState {
    /// The cache of account and storage data
    pub cache: Cache,
    /// The accumulated bundle of state changes
    pub bundle_state: BundleState,
}

const BLOCK_TIME: u64 = 2; // 2 seconds per block

/// Output from metering a bundle of transactions
#[derive(Debug)]
pub struct MeterBundleOutput {
    /// Transaction results with individual metrics
    pub results: Vec<TransactionResult>,
    /// Total gas used by all transactions
    pub total_gas_used: u64,
    /// Total gas fees paid by all transactions
    pub total_gas_fees: U256,
    /// Bundle hash
    pub bundle_hash: B256,
    /// Total execution time in microseconds (includes state root calculation)
    pub total_execution_time_us: u128,
    /// State root calculation time in microseconds
    pub state_root_time_us: u128,
}

/// Simulates and meters a bundle of transactions
///
/// Takes a state provider, chain spec, decoded transactions, block header, and bundle metadata,
/// and executes transactions in sequence to measure gas usage and execution time.
///
/// Returns [`MeterBundleOutput`] containing transaction results and aggregated metrics.
pub fn meter_bundle<SP>(
    state_provider: SP,
    chain_spec: Arc<OpChainSpec>,
    decoded_txs: Vec<op_alloy_consensus::OpTxEnvelope>,
    header: &SealedHeader,
    bundle_with_metadata: &tips_core::types::BundleWithMetadata,
    flashblocks_state: Option<FlashblocksState>,
    cached_flashblock_trie: Option<crate::FlashblockTrieData>,
) -> EyreResult<MeterBundleOutput>
where
    SP: reth_provider::StateProvider,
{
    // Get bundle hash from BundleWithMetadata
    let bundle_hash = bundle_with_metadata.bundle_hash();

    // If we have flashblocks but no cached trie, compute the flashblock trie first
    // (before starting any timers, since we only want to time the bundle's execution and state root)
    let flashblock_trie = if cached_flashblock_trie.is_none() {
        if let Some(ref fb_state) = flashblocks_state {
            let fb_hashed_state = state_provider.hashed_post_state(&fb_state.bundle_state);
            let (_fb_state_root, fb_trie_updates) =
                state_provider.state_root_with_updates(fb_hashed_state.clone())?;
            Some((fb_trie_updates, fb_hashed_state))
        } else {
            None
        }
    } else {
        None
    };

    // Create state database
    let state_db = reth::revm::database::StateProviderDatabase::new(state_provider);

    // If we have flashblocks state, apply both cache and bundle prestate
    let cache_db = if let Some(ref flashblocks) = flashblocks_state {
        CacheDB {
            cache: flashblocks.cache.clone(),
            db: state_db,
        }
    } else {
        CacheDB::new(state_db)
    };

    // Wrap the CacheDB in a State to track bundle changes for state root calculation
    let mut db = if let Some(flashblocks) = flashblocks_state.as_ref() {
        State::builder()
            .with_database(cache_db)
            .with_bundle_update()
            .with_bundle_prestate(flashblocks.bundle_state.clone())
            .build()
    } else {
        State::builder()
            .with_database(cache_db)
            .with_bundle_update()
            .build()
    };

    // Set up next block attributes
    // Use bundle.min_timestamp if provided, otherwise use header timestamp + BLOCK_TIME
    let timestamp = bundle_with_metadata
        .bundle()
        .min_timestamp
        .unwrap_or_else(|| header.timestamp() + BLOCK_TIME);
    let attributes = OpNextBlockEnvAttributes {
        timestamp,
        suggested_fee_recipient: header.beneficiary(),
        prev_randao: header.mix_hash().unwrap_or(B256::random()),
        gas_limit: header.gas_limit(),
        parent_beacon_block_root: header.parent_beacon_block_root(),
        extra_data: header.extra_data().clone(),
    };

    // Execute transactions
    let mut results = Vec::new();
    let mut total_gas_used = 0u64;
    let mut total_gas_fees = U256::ZERO;

    let execution_start = Instant::now();
    {
        let evm_config = OpEvmConfig::optimism(chain_spec);
        let mut builder = evm_config.builder_for_next_block(&mut db, header, attributes)?;

        builder.apply_pre_execution_changes()?;

        for tx in decoded_txs {
            let tx_start = Instant::now();
            let tx_hash = tx.tx_hash();
            let from = tx.recover_signer()?;
            let to = tx.to();
            let value = tx.value();
            let gas_price = tx.max_fee_per_gas();

            let recovered_tx =
                alloy_consensus::transaction::Recovered::new_unchecked(tx.clone(), from);

            let gas_used = builder
                .execute_transaction(recovered_tx)
                .map_err(|e| eyre!("Transaction {} execution failed: {}", tx_hash, e))?;

            let gas_fees = U256::from(gas_used) * U256::from(gas_price);
            total_gas_used = total_gas_used.saturating_add(gas_used);
            total_gas_fees = total_gas_fees.saturating_add(gas_fees);

            let execution_time = tx_start.elapsed().as_micros();

            results.push(TransactionResult {
                coinbase_diff: gas_fees.to_string(),
                eth_sent_to_coinbase: "0".to_string(),
                from_address: from,
                gas_fees: gas_fees.to_string(),
                gas_price: gas_price.to_string(),
                gas_used,
                to_address: to,
                tx_hash,
                value: value.to_string(),
                execution_time_us: execution_time,
            });
        }
    }

    // Calculate state root and measure its calculation time
    db.merge_transitions(BundleRetention::Reverts);
    let bundle = db.take_bundle();
    let state_provider = db.database.db.as_ref();

    let state_root_start = Instant::now();
    let hashed_post_state = state_provider.hashed_post_state(&bundle);

    if let Some(cached) = cached_flashblock_trie {
        // We have cached flashblock trie nodes, use them
        let mut trie_input = TrieInput::from_state(hashed_post_state);
        trie_input.prepend_cached(cached.trie_updates, cached.hashed_state);
        let _ = state_provider.state_root_from_nodes_with_updates(trie_input)?;
    } else if let Some((fb_trie_updates, fb_hashed_state)) = flashblock_trie {
        // We computed the flashblock trie above, now use it
        let mut trie_input = TrieInput::from_state(hashed_post_state);
        trie_input.prepend_cached(fb_trie_updates, fb_hashed_state);
        let _ = state_provider.state_root_from_nodes_with_updates(trie_input)?;
    } else {
        // No flashblocks, just calculate bundle state root
        let _ = state_provider.state_root_with_updates(hashed_post_state)?;
    }

    let state_root_time = state_root_start.elapsed().as_micros();

    let total_execution_time = execution_start.elapsed().as_micros();

    Ok(MeterBundleOutput {
        results,
        total_gas_used,
        total_gas_fees,
        bundle_hash,
        total_execution_time_us: total_execution_time,
        state_root_time_us: state_root_time,
    })
}

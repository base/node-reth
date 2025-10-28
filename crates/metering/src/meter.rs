use alloy_consensus::{transaction::SignerRecoverable, BlockHeader, Transaction as _};
use alloy_primitives::{B256, U256};
use eyre::{eyre, Result as EyreResult};
use reth::revm::db::State;
use reth_evm::execute::BlockBuilder;
use reth_evm::ConfigureEvm;
use reth_optimism_chainspec::OpChainSpec;
use reth_optimism_evm::{OpEvmConfig, OpNextBlockEnvAttributes};
use reth_optimism_primitives::OpPrimitives;
use reth_primitives_traits::SealedHeader;
use reth_provider::ExecutionOutcome;
use std::sync::Arc;
use std::time::Instant;

use crate::TransactionResult;

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
) -> EyreResult<MeterBundleOutput>
where
    SP: reth_provider::StateProvider,
{
    // Get bundle hash from BundleWithMetadata
    let bundle_hash = bundle_with_metadata.bundle_hash();

    // Create state database
    let state_db = reth::revm::database::StateProviderDatabase::new(state_provider);
    let mut db = State::builder()
        .with_database(state_db)
        .with_bundle_update()
        .build();

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
    let block_number = header.number() + 1;
    let bundle = db.take_bundle();
    let execution_outcome: ExecutionOutcome<OpPrimitives> = ExecutionOutcome::new(
        bundle,
        Vec::new().into(),
        block_number,
        Vec::new(),
    );

    let state_provider = db.database.as_ref();
    let state_root_start = Instant::now();
    let hashed_state = state_provider.hashed_post_state(execution_outcome.state());
    let _ = state_provider.state_root_with_updates(hashed_state);
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

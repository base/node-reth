use crate::rpc::{MeteringApiImpl, MeteringApiServer};
use alloy_consensus::Receipt;
use alloy_eips::Encodable2718;
use alloy_primitives::{address, Bytes, B256, U256};
use alloy_provider::Provider;
use base_reth_flashblocks_rpc::rpc::FlashblocksAPI;
use base_reth_test_utils::harness::TestHarness;
use base_reth_test_utils::node::{
    default_launcher, LocalFlashblocksState, LocalNodeProvider, BASE_CHAIN_ID,
};
use eyre::{eyre, Result};
use op_alloy_consensus::OpTxEnvelope;
use reth_optimism_primitives::{OpReceipt, OpTransactionSigned};
use reth_provider::HeaderProvider;
use reth_transaction_pool::test_utils::TransactionBuilder;
use tips_core::types::Bundle;

use super::utils::{build_single_flashblock, secret_from_hex};

struct RpcTestContext {
    harness: TestHarness,
    api: MeteringApiImpl<LocalNodeProvider, LocalFlashblocksState>,
}

impl RpcTestContext {
    async fn new() -> Result<Self> {
        let harness = TestHarness::new(default_launcher).await?;
        let provider = harness.blockchain_provider();
        let flashblocks_state = harness.flashblocks_state();
        let api = MeteringApiImpl::new(provider, flashblocks_state);

        Ok(Self { harness, api })
    }

    fn accounts(&self) -> &base_reth_test_utils::accounts::TestAccounts {
        self.harness.accounts()
    }

    fn harness(&self) -> &TestHarness {
        &self.harness
    }

    async fn meter_bundle(&self, bundle: Bundle) -> Result<crate::MeterBundleResponse> {
        MeteringApiServer::meter_bundle(&self.api, bundle)
            .await
            .map_err(|err| eyre!("meter_bundle rpc failed: {}", err))
    }

    async fn meter_bundle_raw(
        &self,
        bundle: Bundle,
    ) -> jsonrpsee::core::RpcResult<crate::MeterBundleResponse> {
        MeteringApiServer::meter_bundle(&self.api, bundle).await
    }
}

fn create_bundle(txs: Vec<Bytes>, block_number: u64, min_timestamp: Option<u64>) -> Bundle {
    Bundle {
        txs,
        block_number,
        flashblock_number_min: None,
        flashblock_number_max: None,
        min_timestamp,
        max_timestamp: None,
        reverting_tx_hashes: vec![],
        replacement_uuid: None,
        dropping_tx_hashes: vec![],
    }
}

#[tokio::test]
async fn test_meter_bundle_empty() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let bundle = create_bundle(vec![], 0, None);
    let response = ctx.meter_bundle(bundle).await?;

    assert_eq!(response.results.len(), 0);
    assert_eq!(response.total_gas_used, 0);
    assert_eq!(response.gas_fees, "0");

    let latest_block = ctx.harness().provider().get_block_number().await?;
    assert_eq!(response.state_block_number, latest_block);

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_single_transaction() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let sender_account = &ctx.accounts().alice;
    let sender_address = sender_account.address;
    let sender_secret = secret_from_hex(sender_account.private_key);

    let tx = TransactionBuilder::default()
        .signer(sender_secret)
        .chain_id(BASE_CHAIN_ID)
        .nonce(0)
        .to(address!("0x1111111111111111111111111111111111111111"))
        .value(1000)
        .gas_limit(21_000)
        .max_fee_per_gas(1_000_000_000)
        .max_priority_fee_per_gas(1_000_000_000)
        .into_eip1559();

    let signed_tx = OpTransactionSigned::Eip1559(tx.as_eip1559().unwrap().clone());
    let envelope: OpTxEnvelope = signed_tx.into();
    let tx_bytes = Bytes::from(envelope.encoded_2718());

    let bundle = create_bundle(vec![tx_bytes], 0, None);
    let response = ctx.meter_bundle(bundle).await?;

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.total_gas_used, 21_000);
    assert!(response.total_execution_time_us > 0);
    assert!(
        response.state_root_time_us > 0,
        "state_root_time_us should be greater than zero"
    );

    let result = &response.results[0];
    assert_eq!(result.from_address, sender_address);
    assert_eq!(
        result.to_address,
        Some(address!("0x1111111111111111111111111111111111111111"))
    );
    assert_eq!(result.gas_used, 21_000);
    assert_eq!(result.gas_price, "1000000000");
    assert!(result.execution_time_us > 0);

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_multiple_transactions() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let secret1 = secret_from_hex(ctx.accounts().alice.private_key);
    let secret2 = secret_from_hex(ctx.accounts().bob.private_key);

    let tx1 = TransactionBuilder::default()
        .signer(secret1)
        .chain_id(BASE_CHAIN_ID)
        .nonce(0)
        .to(address!("0x1111111111111111111111111111111111111111"))
        .value(1000)
        .gas_limit(21_000)
        .max_fee_per_gas(1_000_000_000)
        .max_priority_fee_per_gas(1_000_000_000)
        .into_eip1559();
    let tx1_bytes = Bytes::from(
        OpTxEnvelope::from(OpTransactionSigned::Eip1559(
            tx1.as_eip1559().unwrap().clone(),
        ))
        .encoded_2718(),
    );

    let tx2 = TransactionBuilder::default()
        .signer(secret2)
        .chain_id(BASE_CHAIN_ID)
        .nonce(0)
        .to(address!("0x2222222222222222222222222222222222222222"))
        .value(2000)
        .gas_limit(21_000)
        .max_fee_per_gas(2_000_000_000)
        .max_priority_fee_per_gas(2_000_000_000)
        .into_eip1559();
    let tx2_bytes = Bytes::from(
        OpTxEnvelope::from(OpTransactionSigned::Eip1559(
            tx2.as_eip1559().unwrap().clone(),
        ))
        .encoded_2718(),
    );

    let bundle = create_bundle(vec![tx1_bytes, tx2_bytes], 0, None);
    let response = ctx.meter_bundle(bundle).await?;

    assert_eq!(response.results.len(), 2);
    assert_eq!(response.total_gas_used, 42_000);

    let result1 = &response.results[0];
    assert_eq!(result1.from_address, ctx.accounts().alice.address);
    assert_eq!(result1.gas_price, "1000000000");

    let result2 = &response.results[1];
    assert_eq!(result2.from_address, ctx.accounts().bob.address);
    assert_eq!(result2.gas_price, "2000000000");

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_invalid_transaction() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let bundle = create_bundle(vec![Bytes::from_static(b"\xde\xad\xbe\xef")], 0, None);
    let result = ctx.meter_bundle_raw(bundle).await;

    assert!(result.is_err(), "expected invalid transaction to fail");
    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_uses_latest_block() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    ctx.harness().advance_chain(2).await?;

    let bundle = create_bundle(vec![], 0, None);
    let response = ctx.meter_bundle(bundle).await?;

    let latest_block = ctx.harness().provider().get_block_number().await?;
    assert_eq!(response.state_block_number, latest_block);

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_ignores_bundle_block_number() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let bundle1 = create_bundle(vec![], 0, None);
    let response1 = ctx.meter_bundle(bundle1).await?;

    let bundle2 = create_bundle(vec![], 999, None);
    let response2 = ctx.meter_bundle(bundle2).await?;

    assert_eq!(response1.state_block_number, response2.state_block_number);
    let latest_block = ctx.harness().provider().get_block_number().await?;
    assert_eq!(response1.state_block_number, latest_block);

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_custom_timestamp() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let custom_timestamp = 1_234_567_890;
    let bundle = create_bundle(vec![], 0, Some(custom_timestamp));
    let response = ctx.meter_bundle(bundle).await?;

    assert_eq!(response.results.len(), 0);
    assert_eq!(response.total_gas_used, 0);

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_arbitrary_block_number() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let bundle = create_bundle(vec![], 999_999, None);
    let response = ctx.meter_bundle(bundle).await?;

    let latest_block = ctx.harness().provider().get_block_number().await?;
    assert_eq!(response.state_block_number, latest_block);

    Ok(())
}

#[tokio::test]
async fn test_meter_bundle_gas_calculations() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let secret1 = secret_from_hex(ctx.accounts().alice.private_key);
    let secret2 = secret_from_hex(ctx.accounts().bob.private_key);

    let tx1 = TransactionBuilder::default()
        .signer(secret1)
        .chain_id(BASE_CHAIN_ID)
        .nonce(0)
        .to(address!("0x1111111111111111111111111111111111111111"))
        .value(1000)
        .gas_limit(21_000)
        .max_fee_per_gas(3_000_000_000)
        .max_priority_fee_per_gas(3_000_000_000)
        .into_eip1559();
    let tx1_bytes = Bytes::from(
        OpTxEnvelope::from(OpTransactionSigned::Eip1559(
            tx1.as_eip1559().unwrap().clone(),
        ))
        .encoded_2718(),
    );

    let tx2 = TransactionBuilder::default()
        .signer(secret2)
        .chain_id(BASE_CHAIN_ID)
        .nonce(0)
        .to(address!("0x2222222222222222222222222222222222222222"))
        .value(2000)
        .gas_limit(21_000)
        .max_fee_per_gas(7_000_000_000)
        .max_priority_fee_per_gas(7_000_000_000)
        .into_eip1559();
    let tx2_bytes = Bytes::from(
        OpTxEnvelope::from(OpTransactionSigned::Eip1559(
            tx2.as_eip1559().unwrap().clone(),
        ))
        .encoded_2718(),
    );

    let bundle = create_bundle(vec![tx1_bytes, tx2_bytes], 0, None);
    let response = ctx.meter_bundle(bundle).await?;

    assert_eq!(response.results.len(), 2);

    let expected_fees_1 = U256::from(21_000) * U256::from(3_000_000_000u64);
    let expected_fees_2 = U256::from(21_000) * U256::from(7_000_000_000u64);

    assert_eq!(response.results[0].gas_fees, expected_fees_1.to_string());
    assert_eq!(response.results[0].gas_price, "3000000000");
    assert_eq!(response.results[1].gas_fees, expected_fees_2.to_string());
    assert_eq!(response.results[1].gas_price, "7000000000");

    let total_fees = expected_fees_1 + expected_fees_2;
    assert_eq!(response.gas_fees, total_fees.to_string());
    assert_eq!(response.coinbase_diff, total_fees.to_string());
    assert_eq!(response.total_gas_used, 42_000);
    assert_eq!(response.bundle_gas_price, "5000000000");

    Ok(())
}

#[tokio::test]
async fn flashblock_without_beacon_root_errors() -> Result<()> {
    reth_tracing::init_test_tracing();
    let ctx = RpcTestContext::new().await?;

    let provider = ctx.harness().provider();
    let latest_block = provider.get_block_number().await?;
    let blockchain_provider = ctx.harness().blockchain_provider();
    let latest_header = blockchain_provider
        .sealed_header(latest_block)?
        .ok_or_else(|| eyre!("missing header for block {}", latest_block))?;

    let alice_secret = secret_from_hex(ctx.accounts().alice.private_key);
    let tx = TransactionBuilder::default()
        .signer(alice_secret)
        .chain_id(BASE_CHAIN_ID)
        .nonce(0)
        .to(ctx.accounts().bob.address)
        .value(1)
        .gas_limit(21_000)
        .max_fee_per_gas(1_000_000_000)
        .max_priority_fee_per_gas(1_000_000_000)
        .into_eip1559();

    let envelope = OpTxEnvelope::from(OpTransactionSigned::Eip1559(
        tx.as_eip1559().unwrap().clone(),
    ));
    let tx_hash = envelope.tx_hash();
    let tx_bytes = Bytes::from(envelope.encoded_2718());
    let receipt = OpReceipt::Eip1559(Receipt {
        status: true.into(),
        cumulative_gas_used: 21_000,
        logs: vec![],
    });

    // Zero-out the parent beacon block root to emulate a flashblock that lacks Cancun data.
    let flashblock = build_single_flashblock(
        latest_header.number + 1,
        latest_header.hash(),
        B256::ZERO,
        latest_header.timestamp + 2,
        latest_header.gas_limit,
        vec![(tx_bytes.clone(), Some((tx_hash, receipt.clone())))],
    );

    ctx.harness().send_flashblock(flashblock).await?;

    let bundle = create_bundle(vec![tx_bytes], latest_header.number + 1, None);
    let err = ctx
        .meter_bundle_raw(bundle)
        .await
        .expect_err("pending flashblock metering should fail without beacon root");
    assert!(err.message().contains("parent beacon block root missing"));

    let pending_blocks = ctx.harness().flashblocks_state().get_pending_blocks();
    assert!(
        pending_blocks.is_some(),
        "flashblock should populate pending state"
    );
    assert_eq!(
        pending_blocks.as_ref().unwrap().latest_flashblock_index(),
        0
    );

    Ok(())
}

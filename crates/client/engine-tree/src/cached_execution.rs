use alloy_consensus::transaction::TxHashRef;
use alloy_op_evm::block::OpTxResult;
use op_alloy_consensus::OpTxType;
use reth_errors::BlockExecutionError;
use reth_evm::{
    ConfigureEvm, Evm, RecoveredTx,
    block::{BlockExecutor, BlockExecutorFor, ExecutableTx, TxResult},
    execute::ExecutableTxFor,
};
use reth_provider::ExecutionOutcome;
use reth_revm::State;
use revm::{Database, context::result::ResultAndState};
use revm_primitives::B256;

pub trait CachedExecutionProvider<Result> {
    // TODO: what do we need to check to ensure the tx execution is valid?
    fn get_cached_execution_for_tx<'a>(
        &self,
        start_state_root: &B256,
        prev_cached_hash: Option<&B256>,
        tx_hash: &B256,
    ) -> Option<Result>;
}

#[derive(Debug, Clone, Default)]
pub struct NoopCachedExecutionProvider;

impl<Result> CachedExecutionProvider<Result> for NoopCachedExecutionProvider {
    fn get_cached_execution_for_tx<'a>(
        &self,
        start_state_root: &B256,
        prev_cached_hash: Option<&B256>,
        tx_hash: &B256,
    ) -> Option<Result> {
        None
    }
}

pub struct CachedExecutor<E, C> {
    executor: E,
    cached_execution_provider: C,
    txs: Vec<B256>,
    block_state_root: B256,
    all_txs_cached: bool,
}

impl<E, C> CachedExecutor<E, C> {
    pub fn new(
        executor: E,
        cached_execution_provider: C,
        txs: Vec<B256>,
        block_state_root: B256,
    ) -> Self {
        Self { executor, cached_execution_provider, txs, block_state_root, all_txs_cached: true }
    }
}

impl<'a, E, C, DB> BlockExecutor for CachedExecutor<E, C>
where
    DB: Database + 'a,
    E: BlockExecutor<Transaction: TxHashRef, Evm: Evm<DB = &'a mut State<DB>>>,
    C: CachedExecutionProvider<E::Result>,
{
    type Transaction = E::Transaction;
    type Receipt = E::Receipt;
    type Evm = E::Evm;
    type Result = E::Result;

    fn receipts(&self) -> &[Self::Receipt] {
        self.executor.receipts()
    }

    fn execute_transaction_without_commit(
        &mut self,
        executing_tx: impl ExecutableTx<Self>,
    ) -> Result<Self::Result, BlockExecutionError> {
        if !self.all_txs_cached {
            return self.executor.execute_transaction_without_commit(executing_tx);
        }

        let (_, executing_tx_recovered) = executing_tx.into_parts();

        // find tx just before this one
        let prev_tx_hash =
            self.txs.iter().take_while(|tx| *tx != executing_tx_recovered.tx().tx_hash()).last();

        let cached_execution = self.cached_execution_provider.get_cached_execution_for_tx(
            &self.block_state_root,
            prev_tx_hash,
            &executing_tx_recovered.tx().tx_hash(),
        );
        if let Some(cached_execution) = cached_execution {
            // load accounts into cache
            for (address, _) in cached_execution.result().state.iter() {
                let _ = self.executor.evm_mut().db_mut().load_cache_account(*address);
            }
            return Ok(cached_execution);
        }
        self.all_txs_cached = false;
        self.executor.execute_transaction_without_commit(executing_tx)
    }

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        self.executor.apply_pre_execution_changes()
    }

    fn commit_transaction(&mut self, output: Self::Result) -> Result<u64, BlockExecutionError> {
        self.executor.commit_transaction(output)
    }

    fn finish(
        self,
    ) -> Result<(Self::Evm, reth_provider::BlockExecutionResult<Self::Receipt>), BlockExecutionError>
    {
        self.executor.finish()
    }

    fn set_state_hook(&mut self, hook: Option<Box<dyn reth_evm::OnStateHook>>) {
        self.executor.set_state_hook(hook)
    }

    fn evm_mut(&mut self) -> &mut Self::Evm {
        self.executor.evm_mut()
    }

    fn evm(&self) -> &Self::Evm {
        self.executor.evm()
    }
}

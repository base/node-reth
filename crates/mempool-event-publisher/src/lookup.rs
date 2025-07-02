use std::sync::Arc;

use alloy_primitives::TxHash;
use reth_transaction_pool::{PoolTransaction, ValidPoolTransaction};

/// Trait for looking up transaction data by hash
///
/// This trait provides a decoupled way to access transaction data, allowing
/// the mempool event publisher to enrich events with transaction details
/// without being directly dependent on the mempool implementation.
#[async_trait::async_trait]
pub trait TransactionLookup: Send + Sync {
    type Transaction: PoolTransaction;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Look up a transaction by its hash
    ///
    /// Returns the transaction if found in the lookup source (mempool, storage, etc.)
    async fn get_transaction(
        &self,
        hash: TxHash,
    ) -> Result<Option<Arc<ValidPoolTransaction<Self::Transaction>>>, Self::Error>;

    /// Check if a transaction exists by its hash (lighter operation than full lookup)
    async fn contains_transaction(&self, hash: TxHash) -> Result<bool, Self::Error> {
        Ok(self.get_transaction(hash).await?.is_some())
    }
}

/// Wrapper for any type that implements TransactionPool
pub struct PoolWrapper<P> {
    pool: P,
}

impl<P> PoolWrapper<P> {
    pub fn new(pool: P) -> Self {
        Self { pool }
    }
}

/// Implementation of TransactionLookup for any TransactionPool
#[async_trait::async_trait]
impl<P> TransactionLookup for PoolWrapper<P>
where
    P: reth_transaction_pool::TransactionPool + Send + Sync,
    P::Transaction: PoolTransaction + Send + Sync + 'static,
{
    type Transaction = P::Transaction;
    type Error = reth_transaction_pool::error::PoolError;

    async fn get_transaction(
        &self,
        hash: TxHash,
    ) -> Result<Option<Arc<ValidPoolTransaction<Self::Transaction>>>, Self::Error> {
        // The get method is synchronous, so we just call it directly
        Ok(self.pool.get(&hash))
    }

    async fn contains_transaction(&self, hash: TxHash) -> Result<bool, Self::Error> {
        Ok(self.pool.get(&hash).is_some())
    }
}

pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    /// Mock implementation of TransactionLookup for testing
    pub struct MockTransactionLookup<T: PoolTransaction> {
        transactions: RwLock<HashMap<TxHash, Arc<ValidPoolTransaction<T>>>>,
    }

    impl<T: PoolTransaction> MockTransactionLookup<T> {
        pub fn new() -> Self {
            Self {
                transactions: RwLock::new(HashMap::new()),
            }
        }

        pub async fn add_transaction(&self, tx: Arc<ValidPoolTransaction<T>>) {
            let hash = tx.transaction.hash();
            self.transactions.write().await.insert(*hash, tx);
        }

        pub async fn remove_transaction(&self, hash: TxHash) {
            self.transactions.write().await.remove(&hash);
        }

        pub async fn clear(&self) {
            self.transactions.write().await.clear();
        }
    }

    impl<T: PoolTransaction> Default for MockTransactionLookup<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait::async_trait]
    impl<T: PoolTransaction> TransactionLookup for MockTransactionLookup<T> {
        type Transaction = T;
        type Error = MockLookupError;

        async fn get_transaction(
            &self,
            hash: TxHash,
        ) -> Result<Option<Arc<ValidPoolTransaction<Self::Transaction>>>, Self::Error> {
            Ok(self.transactions.read().await.get(&hash).cloned())
        }

        async fn contains_transaction(&self, hash: TxHash) -> Result<bool, Self::Error> {
            Ok(self.transactions.read().await.contains_key(&hash))
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("Mock lookup error: {message}")]
    pub struct MockLookupError {
        pub message: String,
    }
}

#[cfg(test)]
mod tests {
    use super::mock::*;
    use super::*;
    use alloy_primitives::TxHash;

    #[tokio::test]
    async fn test_mock_transaction_lookup() {
        let lookup = MockTransactionLookup::<reth_transaction_pool::EthPooledTransaction>::new();
        let hash = TxHash::from([1u8; 32]);

        // Initially empty
        assert!(!lookup.contains_transaction(hash).await.unwrap());
        assert!(lookup.get_transaction(hash).await.unwrap().is_none());

        // Note: We can't easily create a ValidPoolTransaction in tests without more setup
        // This test demonstrates the interface, real tests would need transaction creation helpers
    }
}

use std::sync::Arc;

use alloy_primitives::TxHash;
use chrono::{DateTime, Utc};
use reth_transaction_pool::{FullTransactionEvent, PoolTransaction, ValidPoolTransaction};
use serde::{Deserialize, Serialize};

/// Transaction data payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionPayload {
    /// Transaction hash
    pub transaction_hash: String,
    /// Sender address (if available for events with transaction data)
    pub from: Option<String>,
    /// Recipient address (if available for events with transaction data)
    pub to: Option<String>,
    /// Transaction value in wei (if available for events with transaction data)
    pub value: Option<String>,
    /// Gas price in wei (if available for events with transaction data)
    pub gas_price: Option<String>,
    /// Gas limit (if available for events with transaction data)
    pub gas_limit: Option<u64>,
    /// Transaction nonce (if available for events with transaction data)
    pub nonce: Option<u64>,
    /// Which subpool the transaction belongs to
    pub subpool: Option<String>,
    /// Chain ID (if available for events with transaction data)
    pub chain_id: Option<u64>,
    /// Transaction type (Legacy, EIP1559, etc.) (if available for events with transaction data)
    pub tx_type: Option<u8>,
    /// Block hash for mined transactions
    pub block_hash: Option<String>,
    /// Hash of the transaction that replaced this one
    pub replaced_by: Option<String>,
}

/// Represents a mempool event that will be published to Kafka
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolEvent {
    /// Type of the event
    pub event_type: String,
    /// Timestamp when the event occurred
    pub timestamp: DateTime<Utc>,
    /// Transaction data payload
    pub payload: TransactionPayload,
}

impl MempoolEvent {
    /// Convert a Reth FullTransactionEvent to a MempoolEvent
    pub fn from_reth_event<T: PoolTransaction>(event: &FullTransactionEvent<T>) -> Self {
        let timestamp = Utc::now();
        
        match event {
            FullTransactionEvent::Pending(tx_hash) => Self {
                event_type: "pending".to_string(),
                timestamp,
                payload: TransactionPayload {
                    transaction_hash: format!("{:?}", tx_hash),
                    from: None,
                    to: None,
                    value: None,
                    gas_price: None,
                    gas_limit: None,
                    nonce: None,
                    subpool: Some("pending".to_string()),
                    chain_id: None,
                    tx_type: None,
                    block_hash: None,
                    replaced_by: None,
                },
            },
            FullTransactionEvent::Queued(tx_hash) => Self {
                event_type: "queued".to_string(),
                timestamp,
                payload: TransactionPayload {
                    transaction_hash: format!("{:?}", tx_hash),
                    from: None,
                    to: None,
                    value: None,
                    gas_price: None,
                    gas_limit: None,
                    nonce: None,
                    subpool: Some("queued".to_string()),
                    chain_id: None,
                    tx_type: None,
                    block_hash: None,
                    replaced_by: None,
                },
            },
            FullTransactionEvent::Mined {
                tx_hash,
                block_hash,
            } => Self {
                event_type: "mined".to_string(),
                timestamp,
                payload: TransactionPayload {
                    transaction_hash: format!("{:?}", tx_hash),
                    from: None,
                    to: None,
                    value: None,
                    gas_price: None,
                    gas_limit: None,
                    nonce: None,
                    subpool: None,
                    chain_id: None,
                    tx_type: None,
                    block_hash: Some(format!("{:?}", block_hash)),
                    replaced_by: None,
                },
            },
            FullTransactionEvent::Replaced { transaction, replaced_by: _ } => {
                let replaced_tx_hash = transaction.transaction.hash();
                Self {
                    event_type: "replaced".to_string(),
                    timestamp,
                    payload: TransactionPayload {
                        transaction_hash: format!("{:?}", replaced_tx_hash),
                        from: None,
                        to: None,
                        value: None,
                        gas_price: None,
                        gas_limit: None,
                        nonce: None,
                        subpool: None,
                        chain_id: None,
                        tx_type: None,
                        block_hash: None,
                        replaced_by: Some(format!("{:?}", replaced_tx_hash)),
                    },
                }
            }
            FullTransactionEvent::Discarded(tx_hash) => Self {
                event_type: "discarded".to_string(),
                timestamp,
                payload: TransactionPayload {
                    transaction_hash: format!("{:?}", tx_hash),
                    from: None,
                    to: None,
                    value: None,
                    gas_price: None,
                    gas_limit: None,
                    nonce: None,
                    subpool: None,
                    chain_id: None,
                    tx_type: None,
                    block_hash: None,
                    replaced_by: None,
                },
            },
            FullTransactionEvent::Invalid(tx_hash) => Self {
                event_type: "invalid".to_string(),
                timestamp,
                payload: TransactionPayload {
                    transaction_hash: format!("{:?}", tx_hash),
                    from: None,
                    to: None,
                    value: None,
                    gas_price: None,
                    gas_limit: None,
                    nonce: None,
                    subpool: None,
                    chain_id: None,
                    tx_type: None,
                    block_hash: None,
                    replaced_by: None,
                },
            },
            FullTransactionEvent::Propagated(_) => Self {
                event_type: "propagated".to_string(),
                timestamp,
                payload: TransactionPayload {
                    transaction_hash: "unknown".to_string(),
                    from: None,
                    to: None,
                    value: None,
                    gas_price: None,
                    gas_limit: None,
                    nonce: None,
                    subpool: None,
                    chain_id: None,
                    tx_type: None,
                    block_hash: None,
                    replaced_by: None,
                },
            },
        }
    }

    /// Generate a Kafka message key from the transaction hash
    pub fn kafka_key(&self) -> String {
        self.payload.transaction_hash.clone()
    }

    /// Get the transaction hash for lookup purposes
    pub fn get_transaction_hash(&self) -> Option<TxHash> {
        if self.payload.transaction_hash == "unknown" {
            return None;
        }

        let hash_str = self.payload.transaction_hash.strip_prefix("0x")
            .unwrap_or(&self.payload.transaction_hash);
        
        hex::decode(hash_str)
            .ok()
            .and_then(|bytes| {
                if bytes.len() == 32 {
                    let mut hash_bytes = [0u8; 32];
                    hash_bytes.copy_from_slice(&bytes);
                    Some(TxHash::from(hash_bytes))
                } else {
                    None
                }
            })
    }

    /// Enrich the event with transaction data from the pool
    pub fn enrich_with_transaction_data<T: PoolTransaction>(&mut self, tx_data: &Arc<ValidPoolTransaction<T>>) {
        let tx = &tx_data.transaction;
        
        self.payload.from = Some(format!("{:?}", tx.sender()));
        
        if let Some(to_addr) = tx.to() {
            self.payload.to = Some(format!("{:?}", to_addr));
        }
        
        self.payload.value = Some(tx.value().to_string());
        self.payload.gas_price = Some(tx.max_fee_per_gas().to_string());
        self.payload.gas_limit = Some(tx.gas_limit());
        self.payload.nonce = Some(tx.nonce());
        self.payload.chain_id = Some(tx.chain_id().unwrap_or(0));
    }

    /// Serialize the event to JSON for Kafka publishing
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{TxHash, BlockHash};

    #[test]
    fn test_mempool_event_serialization() {
        let event = MempoolEvent {
            event_type: "pending".to_string(),
            timestamp: Utc::now(),
            payload: TransactionPayload {
                transaction_hash: "0x1234567890abcdef".to_string(),
                from: Some("0xabcdef1234567890".to_string()),
                to: Some("0x9876543210fedcba".to_string()),
                value: Some("1000000000000000000".to_string()),
                gas_price: Some("20000000000".to_string()),
                gas_limit: Some(21000),
                nonce: Some(42),
                subpool: Some("pending".to_string()),
                chain_id: Some(1),
                tx_type: Some(2),
                block_hash: None,
                replaced_by: None,
            },
        };

        let json = event.to_json().expect("Should serialize to JSON");
        assert!(json.contains("pending"));
        assert!(json.contains("0x1234567890abcdef"));
        assert!(json.contains("payload"));

        let deserialized: MempoolEvent = serde_json::from_str(&json).expect("Should deserialize from JSON");
        assert_eq!(deserialized.event_type, "pending");
        assert_eq!(deserialized.payload.transaction_hash, "0x1234567890abcdef");
    }

    #[test]
    fn test_kafka_key_generation() {
        let event = MempoolEvent {
            event_type: "pending".to_string(),
            timestamp: Utc::now(),
            payload: TransactionPayload {
                transaction_hash: "0x1234567890abcdef".to_string(),
                from: None,
                to: None,
                value: None,
                gas_price: None,
                gas_limit: None,
                nonce: None,
                subpool: None,
                chain_id: None,
                tx_type: None,
                block_hash: None,
                replaced_by: None,
            },
        };

        let key = event.kafka_key();
        assert_eq!(key, "0x1234567890abcdef");
    }

    #[test]
    fn test_from_reth_event() {
        let tx_hash = TxHash::from_slice(&[1u8; 32]);
        let reth_event = FullTransactionEvent::<reth_transaction_pool::EthPooledTransaction>::Pending(tx_hash);
        
        let mempool_event = MempoolEvent::from_reth_event(&reth_event);
        
        assert_eq!(mempool_event.event_type, "pending");
        assert_eq!(mempool_event.payload.subpool, Some("pending".to_string()));
        assert!(mempool_event.payload.transaction_hash.contains("0x0101"));
    }

    #[test]
    fn test_mined_event_with_block_hash() {
        let tx_hash = TxHash::from_slice(&[1u8; 32]);
        let block_hash = BlockHash::from_slice(&[2u8; 32]);
        
        let reth_event = FullTransactionEvent::<reth_transaction_pool::EthPooledTransaction>::Mined { tx_hash, block_hash };
        let mempool_event = MempoolEvent::from_reth_event(&reth_event);
        
        assert_eq!(mempool_event.event_type, "mined");
        assert!(mempool_event.payload.block_hash.is_some());
        assert!(mempool_event.payload.block_hash.unwrap().contains("0x0202"));
    }
}

use std::{sync::Arc, time::Duration};

use futures::{StreamExt};
use rdkafka::{
    config::ClientConfig,
    producer::{FutureProducer, FutureRecord, Producer},
};
use reth_transaction_pool::{AllTransactionsEvents, FullTransactionEvent};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::{config::KafkaConfig, error::Result, event::MempoolEvent, lookup::TransactionLookup};

/// Publishes mempool events to Kafka
pub struct MempoolEventPublisher<L> {
    producer: FutureProducer,
    config: KafkaConfig,
    lookup: Arc<L>,
}

impl<L> MempoolEventPublisher<L>
where
    L: TransactionLookup,
{
    /// Create a new mempool event publisher
    pub fn new(config: KafkaConfig, lookup: Arc<L>) -> Result<Self> {
        let producer = Self::create_producer(&config)?;

        Ok(Self {
            producer,
            config,
            lookup,
        })
    }

    /// Create a Kafka producer from the configuration
    fn create_producer(config: &KafkaConfig) -> Result<FutureProducer> {
        let mut client_config = ClientConfig::new();

        client_config
            .set("bootstrap.servers", &config.brokers)
            .set("client.id", &config.client_id)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.messages", "100000")
            .set("queue.buffering.max.kbytes", "1048576")
            .set(
                "batch.num.messages",
                config.batch_size.unwrap_or(100).to_string(),
            )
            .set("linger.ms", config.linger_ms.unwrap_or(5).to_string());

        if let Some(compression) = &config.compression {
            client_config.set("compression.type", compression);
        }

        // Apply additional configuration
        if let Some(additional_config) = &config.additional_config {
            for (key, value) in additional_config {
                client_config.set(key, value);
            }
        }

        let producer: FutureProducer =
            client_config.create().map_err(crate::error::Error::Kafka)?;

        Ok(producer)
    }

    /// Start listening to mempool events and publishing them to Kafka
    pub async fn start(&self, events: AllTransactionsEvents<L::Transaction>) -> Result<()> {
        self.start_with_events(events).await
    }

    /// Start with a generic event stream (useful for testing)
    pub async fn start_with_stream<S>(&self, events: S) -> Result<()>
    where
        S: futures::Stream<Item = FullTransactionEvent<L::Transaction>> + Unpin + Send,
    {
        self.start_with_events(events).await
    }

    /// Internal method to handle event processing
    async fn start_with_events<S>(&self, mut events: S) -> Result<()>
    where
        S: futures::Stream<Item = FullTransactionEvent<L::Transaction>> + Unpin + Send,
    {
        info!(
            "Starting mempool event publisher for topic: {}",
            self.config.topic
        );

        let mut stats_interval = interval(Duration::from_secs(60));
        let mut event_count = 0u64;
        let mut error_count = 0u64;

        loop {
            tokio::select! {
                // Handle incoming mempool events
                event = events.next() => {
                    match event {
                        Some(full_event) => {
                            event_count += 1;

                            if let Err(e) = self.publish_event(&full_event).await {
                                error_count += 1;
                                error!("Failed to publish event: {}", e);
                            }
                        }
                        None => {
                            warn!("Mempool event stream ended");
                            break;
                        }
                    }
                }

                // Log statistics periodically
                _ = stats_interval.tick() => {
                    info!(
                        "Mempool event publisher stats: {} events processed, {} errors",
                        event_count,
                        error_count
                    );
                }
            }
        }

        Ok(())
    }

    /// Publish a single mempool event to Kafka
    async fn publish_event(
        &self,
        event: &reth_transaction_pool::FullTransactionEvent<L::Transaction>,
    ) -> Result<()> {
        // Create base event from Reth event
        let mut mempool_event = MempoolEvent::from_reth_event(event);

        // Try to enrich with transaction details using lookup
        if let Some(tx_hash) = mempool_event.get_transaction_hash() {
            match self.lookup.get_transaction(tx_hash).await {
                Ok(Some(tx_data)) => {
                    mempool_event.enrich_with_transaction_data(&tx_data);
                    debug!(
                        "Enriched event for tx: {} with full transaction data",
                        mempool_event.payload.transaction_hash
                    );
                }
                Ok(None) => {
                    debug!("Transaction not found in lookup for hash: {}", tx_hash);
                }
                Err(e) => {
                    warn!("Failed to lookup transaction {}: {}", tx_hash, e);
                }
            }
        }

        let json_payload = mempool_event.to_json()?;
        let key = mempool_event.kafka_key();

        debug!(
            "Publishing event: {} for tx: {}",
            mempool_event.event_type, mempool_event.payload.transaction_hash
        );

        let record = FutureRecord::to(&self.config.topic)
            .key(&key)
            .payload(&json_payload);

        match self.producer.send(record, Duration::from_secs(5)).await {
            Ok(_) => {
                debug!(
                    "Successfully published event for tx: {}",
                    mempool_event.payload.transaction_hash
                );
                Ok(())
            }
            Err((kafka_error, _)) => {
                error!(
                    "Failed to publish event for tx: {}: {}",
                    mempool_event.payload.transaction_hash, kafka_error
                );
                Err(crate::error::Error::Kafka(kafka_error))
            }
        }
    }

    /// Flush any pending messages and close the producer
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down mempool event publisher");

        // Flush any pending messages
        self.producer.flush(Duration::from_secs(10))?;

        Ok(())
    }

    /// Get producer statistics
    pub fn get_stats(&self) -> String {
        // Return a simple status message since statistics() is not available in this version
        "Producer is running".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KafkaConfig;

    #[test]
    fn test_publisher_creation() {
        use super::super::lookup::mock::MockTransactionLookup;
        use std::sync::Arc;

        let config = KafkaConfig::new("localhost:9092", "test-topic");
        let lookup = Arc::new(MockTransactionLookup::<
            reth_transaction_pool::EthPooledTransaction,
        >::new());

        // Creating a publisher should succeed even without a broker
        // The connection is only tested when actually sending messages
        let result = MempoolEventPublisher::new(config, lookup);

        // Producer creation should succeed
        assert!(result.is_ok());
    }

    #[test]
    fn test_producer_config() {
        let config = KafkaConfig::new("localhost:9092", "test-topic")
            .with_client_id("test-client")
            .with_compression("gzip")
            .with_batch_size(50)
            .with_linger_ms(10);

        assert_eq!(config.brokers, "localhost:9092");
        assert_eq!(config.topic, "test-topic");
        assert_eq!(config.client_id, "test-client");
        assert_eq!(config.compression, Some("gzip".to_string()));
        assert_eq!(config.batch_size, Some(50));
        assert_eq!(config.linger_ms, Some(10));
    }
}
